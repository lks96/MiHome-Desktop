use crate::{errors::AppError, models::AppSettings};
use sha2::{Digest, Sha384};
use sqlx::{
    SqlitePool,
    migrate::{Migrate, MigrateError, Migrator},
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
};
use std::{borrow::Cow, path::Path};

static MIGRATOR: Migrator = sqlx::migrate!("./src/database/migrations");

#[derive(Clone)]
pub struct Database {
    pub(crate) pool: SqlitePool,
}
impl Database {
    pub async fn open(path: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(4)
            .connect_with(options)
            .await?;
        let mut connection = pool.acquire().await?;
        connection.ensure_migrations_table().await?;
        let applied = connection.list_applied_migrations().await?;
        let mut migrations = MIGRATOR.migrations.clone();
        for migration in migrations.to_mut() {
            // Versions 1–5 shipped with both LF and CRLF, with identical SQL.
            // Accept either known checksum without rewriting the user's history.
            if !(1..=5).contains(&migration.version) {
                continue;
            }
            let lf = migration.sql.replace("\r\n", "\n");
            let crlf_checksum = Sha384::digest(lf.replace('\n', "\r\n").as_bytes());
            if applied.iter().any(|entry| {
                entry.version == migration.version && entry.checksum.as_ref() == &crlf_checksum[..]
            }) {
                migration.checksum = Cow::Owned(crlf_checksum.to_vec());
            } else if migration.sql.as_ref() != lf {
                migration.checksum = Cow::Owned(Sha384::digest(lf.as_bytes()).to_vec());
                migration.sql = Cow::Owned(lf);
            }
        }
        Migrator {
            migrations,
            ..Migrator::DEFAULT
        }
        .run_direct(&mut *connection)
        .await?;
        drop(connection);
        Ok(Self { pool })
    }
    pub async fn settings(&self) -> Result<AppSettings, AppError> {
        let value: Option<String> =
            sqlx::query_scalar("SELECT value FROM app_settings WHERE key = 'preferences'")
                .fetch_optional(&self.pool)
                .await
                .map_err(|_| AppError::database("load"))?;
        match value {
            Some(value) => {
                let settings: AppSettings =
                    serde_json::from_str(&value).map_err(|_| AppError::database("decode"))?;
                settings
                    .validate()
                    .map_err(|_| AppError::database("validate_stored"))?;
                Ok(settings)
            }
            None => Ok(AppSettings::default()),
        }
    }
    pub async fn save_settings(&self, settings: AppSettings) -> Result<AppSettings, AppError> {
        settings.validate()?;
        let json = serde_json::to_string(&settings).map_err(|_| AppError::database("encode"))?;
        sqlx::query("INSERT INTO app_settings(key, value, updated_at) VALUES('preferences', ?, datetime('now')) ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at")
            .bind(json).execute(&self.pool).await.map_err(|_| AppError::database("save"))?;
        Ok(settings)
    }
    pub async fn close(&self) {
        self.pool.close().await;
    }
}

pub fn startup_error_message(error: &(dyn std::error::Error + 'static)) -> String {
    fn sqlite_code(error: &sqlx::Error) -> String {
        error
            .as_database_error()
            .and_then(|error| error.code())
            .map_or_else(String::new, |code| format!("（SQLite {code}）"))
    }

    match error.downcast_ref::<MigrateError>() {
        Some(MigrateError::VersionMissing(version)) => {
            format!("数据库来自不兼容的版本（缺少迁移 {version}）")
        }
        Some(MigrateError::VersionMismatch(version)) => format!("数据库迁移 {version} 校验不匹配"),
        Some(MigrateError::Dirty(version)) => format!("数据库迁移 {version} 未完成"),
        Some(MigrateError::ExecuteMigration(error, version)) => {
            format!("无法执行数据库迁移 {version}{}", sqlite_code(error))
        }
        Some(MigrateError::Execute(error)) => {
            format!("无法读取数据库迁移记录{}", sqlite_code(error))
        }
        _ => format!(
            "无法打开本地数据库{}",
            error
                .downcast_ref::<sqlx::Error>()
                .map(sqlite_code)
                .unwrap_or_default()
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Theme;
    #[tokio::test]
    async fn reopens_databases_from_both_release_builds_and_preserves_history() {
        for crlf in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("legacy.sqlite");
            let pool = SqlitePoolOptions::new()
                .connect_with(
                    SqliteConnectOptions::new()
                        .filename(&path)
                        .create_if_missing(true),
                )
                .await
                .unwrap();
            let migrations = MIGRATOR
                .iter()
                .map(|migration| {
                    let lf = migration.sql.replace("\r\n", "\n");
                    let sql = if crlf { lf.replace('\n', "\r\n") } else { lf };
                    sqlx::migrate::Migration::new(
                        migration.version,
                        migration.description.clone(),
                        migration.migration_type,
                        Cow::Owned(sql),
                        migration.no_tx,
                    )
                })
                .collect();
            Migrator {
                migrations: Cow::Owned(migrations),
                ..Migrator::DEFAULT
            }
            .run(&pool)
            .await
            .unwrap();
            let old_database = Database { pool };
            let settings = AppSettings {
                theme: Theme::Dark,
                ..AppSettings::default()
            };
            old_database.save_settings(settings.clone()).await.unwrap();
            let query = "SELECT checksum FROM _sqlx_migrations ORDER BY version";
            let history: Vec<Vec<u8>> = sqlx::query_scalar(query)
                .fetch_all(&old_database.pool)
                .await
                .unwrap();
            old_database.close().await;

            let reopened = Database::open(&path).await.unwrap();
            assert_eq!(reopened.settings().await.unwrap(), settings);
            assert_eq!(
                sqlx::query_scalar::<_, Vec<u8>>(query)
                    .fetch_all(&reopened.pool)
                    .await
                    .unwrap(),
                history
            );
            sqlx::query("UPDATE _sqlx_migrations SET checksum = zeroblob(48) WHERE version = 1")
                .execute(&reopened.pool)
                .await
                .unwrap();
            reopened.close().await;
            let error = Database::open(&path)
                .await
                .err()
                .expect("unknown checksum should fail");
            assert!(matches!(
                error.downcast_ref::<MigrateError>(),
                Some(MigrateError::VersionMismatch(1))
            ));
        }
    }

    #[tokio::test]
    async fn preferences_survive_reopen_and_invalid_updates_preserve_saved_value() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.sqlite");
        let db = Database::open(&path).await.unwrap();
        assert_eq!(db.settings().await.unwrap(), AppSettings::default());
        let expected = AppSettings {
            theme: Theme::Dark,
            request_timeout: 25,
            auto_connect: false,
            auto_reconnect: true,
            close_to_tray: false,
            auto_start: false,
        };
        db.save_settings(expected.clone()).await.unwrap();
        let invalid = AppSettings {
            request_timeout: 0,
            ..expected.clone()
        };
        assert_eq!(
            db.save_settings(invalid).await.unwrap_err().code,
            "INVALID_INPUT"
        );
        db.close().await;
        let reopened = Database::open(&path).await.unwrap();
        assert_eq!(reopened.settings().await.unwrap(), expected);
        let mode: String = sqlx::query_scalar("PRAGMA journal_mode")
            .fetch_one(&reopened.pool)
            .await
            .unwrap();
        assert_eq!(mode, "wal");
        reopened.close().await;
    }
}
