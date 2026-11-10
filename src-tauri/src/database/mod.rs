use crate::{errors::AppError, models::AppSettings};
use sqlx::{
    SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
};
use std::path::Path;

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
        sqlx::migrate!("./src/database/migrations")
            .run(&pool)
            .await?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Theme;
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
