use crate::{database::Database, errors::AppError, security::CredentialStore};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use url::Url;
use zeroize::Zeroizing;

#[derive(Clone, Serialize, Deserialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionSummary {
    pub id: String,
    pub name: String,
    pub base_url: String,
}
#[derive(sqlx::FromRow)]
pub struct StoredConnection {
    pub id: String,
    pub name: String,
    pub base_url: String,
    pub credential_ref: String,
}
impl StoredConnection {
    pub fn summary(&self) -> ConnectionSummary {
        ConnectionSummary {
            id: self.id.clone(),
            name: self.name.clone(),
            base_url: self.base_url.clone(),
        }
    }
}

pub fn normalize_url(input: &str) -> Result<Url, AppError> {
    let mut url = Url::parse(input.trim())
        .map_err(|_| AppError::invalid_input("请输入完整的 Home Assistant HTTP 或 HTTPS 地址"))?;
    if !["http", "https"].contains(&url.scheme())
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.path().contains("//")
        || url.path().contains("%")
    {
        return Err(AppError::invalid_input(
            "地址必须为 HTTP/HTTPS，不能包含账号密码、查询参数或片段",
        ));
    }
    let path = format!("{}/", url.path().trim_end_matches('/'));
    url.set_path(&path);
    Ok(url)
}
fn validate_token(token: &str) -> Result<(), AppError> {
    if token.trim().is_empty()
        || token.len() > 8192
        || token.chars().any(|c| c.is_whitespace() || c.is_control())
    {
        return Err(AppError::invalid_input("访问令牌不能为空或包含空白字符"));
    }
    Ok(())
}
pub async fn test_connection(base_url: &str, token: &str, timeout: u32) -> Result<(), AppError> {
    let url = normalize_url(base_url)?;
    validate_token(token)?;
    let url = url
        .join("api/")
        .map_err(|_| AppError::invalid_input("地址格式错误"))?;
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(timeout.clamp(1, 120) as u64))
        .build()
        .map_err(|_| AppError::new("HA_UNREACHABLE", "无法初始化安全连接", true))?;
    let response = client
        .get(url)
        .bearer_auth(token)
        .send()
        .await
        .map_err(|_| {
            AppError::new(
                "HA_UNREACHABLE",
                "无法访问 Home Assistant，请检查地址、网络和证书",
                true,
            )
        })?;
    if response.status() == 401 || response.status() == 403 {
        return Err(AppError::new(
            "HA_AUTH_INVALID",
            "访问令牌无效或权限不足",
            false,
        ));
    }
    if !response.status().is_success() {
        return Err(AppError::new(
            "HA_UNREACHABLE",
            "Home Assistant 返回异常状态，请使用实际 API 地址并避免重定向",
            true,
        ));
    }
    // Bound response size. Remote HTML, errors, or arbitrary JSON are not proof of HA readiness.
    let mut body = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk
            .map_err(|_| AppError::new("HA_UNREACHABLE", "读取 Home Assistant 响应失败", true))?;
        if body.len() + chunk.len() > 65536 {
            return Err(protocol_error());
        }
        body.extend_from_slice(&chunk);
    }
    let json: serde_json::Value = serde_json::from_slice(&body).map_err(|_| protocol_error())?;
    if json.get("message").and_then(|value| value.as_str()) != Some("API running.") {
        return Err(protocol_error());
    }
    Ok(())
}
pub fn protocol_error() -> AppError {
    AppError::new(
        "HA_SYNC_FAILED",
        "Home Assistant 响应格式或订阅结果异常",
        true,
    )
}

impl Database {
    pub async fn connection(&self) -> Result<Option<StoredConnection>, AppError> {
        sqlx::query_as(
            "SELECT id, name, base_url, credential_ref FROM ha_connections WHERE id = 'primary'",
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|_| AppError::database("load_connection"))
    }
    pub async fn save_connection(
        &self,
        store: &dyn CredentialStore,
        name: &str,
        base_url: &str,
        token: Zeroizing<String>,
    ) -> Result<ConnectionSummary, AppError> {
        let name = name.trim();
        if name.is_empty() || name.chars().count() > 80 || name.chars().any(char::is_control) {
            return Err(AppError::invalid_input("连接名称需要 1 至 80 个字符"));
        }
        let url = normalize_url(base_url)?;
        let settings = self.settings().await?;
        test_connection(url.as_str(), &token, settings.request_timeout).await?;
        let previous = self.connection().await?;
        let reference = format!("ha-{}", uuid::Uuid::new_v4());
        store.write(&reference, &token)?;
        let result = sqlx::query("INSERT INTO ha_connections(id, name, base_url, credential_ref) VALUES('primary', ?, ?, ?) ON CONFLICT(id) DO UPDATE SET name=excluded.name, base_url=excluded.base_url, credential_ref=excluded.credential_ref, updated_at=datetime('now')")
            .bind(name).bind(url.as_str()).bind(&reference).execute(&self.pool).await;
        if result.is_err() {
            let _ = store.delete(&reference);
            return Err(AppError::database("save_connection"));
        }
        if let Some(previous) = previous {
            if store.delete(&previous.credential_ref).is_err() {
                tracing::warn!("Unable to remove superseded credential");
            }
        }
        Ok(ConnectionSummary {
            id: "primary".into(),
            name: name.into(),
            base_url: url.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };
    async fn server(
        status: &str,
        body: &str,
        count: usize,
    ) -> (String, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/", listener.local_addr().unwrap());
        let reply = format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let handle = tokio::spawn(async move {
            for _ in 0..count {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut request = Vec::new();
                loop {
                    let mut chunk = [0; 1024];
                    let len = stream.read(&mut chunk).await.unwrap();
                    assert!(len > 0);
                    request.extend_from_slice(&chunk[..len]);
                    if request.windows(4).any(|window| window == b"\r\n\r\n") {
                        break;
                    }
                    assert!(request.len() < 16384);
                }
                let request = String::from_utf8(request).unwrap();
                assert!(request.starts_with("GET /api/ HTTP/1.1"));
                assert!(
                    request
                        .to_ascii_lowercase()
                        .contains("authorization: bearer test-only-token")
                );
                stream.write_all(reply.as_bytes()).await.unwrap();
            }
        });
        (url, handle)
    }
    #[test]
    fn rejects_unsafe_endpoints_and_preserves_subpath() {
        for value in [
            "file:///tmp/test",
            "https://user:password@example.test",
            "https://example.test/?token=x",
            "https://example.test/#x",
            "not-a-url",
        ] {
            assert!(normalize_url(value).is_err());
        }
        assert_eq!(
            normalize_url(" https://example.test/ha ")
                .unwrap()
                .join("api/")
                .unwrap()
                .as_str(),
            "https://example.test/ha/api/"
        );
    }
    #[tokio::test]
    async fn api_probe_requires_auth_and_ha_response() {
        for (status, body, expected) in [
            ("401 Unauthorized", "{}", "HA_AUTH_INVALID"),
            ("200 OK", "{}", "HA_SYNC_FAILED"),
            ("200 OK", "<html>login</html>", "HA_SYNC_FAILED"),
            (
                "302 Found\r\nLocation: http://127.0.0.1:1/",
                "{}",
                "HA_UNREACHABLE",
            ),
        ] {
            let (url, handle) = server(status, body, 1).await;
            assert_eq!(
                test_connection(&url, "test-only-token", 2)
                    .await
                    .unwrap_err()
                    .code,
                expected
            );
            handle.await.unwrap();
        }
        let (url, handle) = server("200 OK", r#"{"message":"API running."}"#, 1).await;
        test_connection(&url, "test-only-token", 2).await.unwrap();
        handle.await.unwrap();
    }
    #[tokio::test]
    async fn saves_credential_reference_and_rotates_without_storing_token() {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::open(&dir.path().join("ha.sqlite")).await.unwrap();
        let store = crate::security::MemoryCredentials::default();
        let (url, handle) = server("200 OK", r#"{"message":"API running."}"#, 2).await;
        db.save_connection(
            &store,
            "Home",
            &url,
            Zeroizing::new("test-only-token".into()),
        )
        .await
        .unwrap();
        let first = db.connection().await.unwrap().unwrap();
        db.save_connection(
            &store,
            "Updated",
            &url,
            Zeroizing::new("test-only-token".into()),
        )
        .await
        .unwrap();
        let second = db.connection().await.unwrap().unwrap();
        assert_ne!(first.credential_ref, second.credential_ref);
        assert!(store.read(&first.credential_ref).is_err());
        assert_eq!(
            store.read(&second.credential_ref).unwrap().as_str(),
            "test-only-token"
        );
        assert!(
            !serde_json::to_string(&second.summary())
                .unwrap()
                .contains("test-only-token")
        );
        let values: (String, String, String) =
            sqlx::query_as("SELECT name, base_url, credential_ref FROM ha_connections")
                .fetch_one(&db.pool)
                .await
                .unwrap();
        assert!(!format!("{values:?}").contains("test-only-token"));
        handle.await.unwrap();
        db.close().await;
    }
}

#[cfg(test)]
mod credential_failures {
    use super::*;
    struct Unavailable;
    impl CredentialStore for Unavailable {
        fn read(&self, _: &str) -> Result<Zeroizing<String>, AppError> {
            unreachable!()
        }
        fn write(&self, _: &str, _: &str) -> Result<(), AppError> {
            Err(AppError::new(
                "CREDENTIAL_STORE_FAILED",
                "Unavailable",
                false,
            ))
        }
        fn delete(&self, _: &str) -> Result<(), AppError> {
            unreachable!()
        }
    }
    #[tokio::test]
    async fn failed_credential_write_does_not_replace_connection() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let dir = tempfile::tempdir().unwrap();
        let db = Database::open(&dir.path().join("failure.sqlite"))
            .await
            .unwrap();
        sqlx::query("INSERT INTO ha_connections(id, name, base_url, credential_ref) VALUES('primary','Existing','http://127.0.0.1:8123/','existing-ref')").execute(&db.pool).await.unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut input = [0; 8192];
            let _ = socket.read(&mut input).await.unwrap();
            let body = r#"{"message":"API running."}"#;
            socket
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
        });
        let result = db
            .save_connection(
                &Unavailable,
                "Replacement",
                &url,
                Zeroizing::new("test-only-token".into()),
            )
            .await;
        assert!(matches!(
            result,
            Err(AppError {
                code: "CREDENTIAL_STORE_FAILED",
                ..
            })
        ));
        assert_eq!(
            db.connection().await.unwrap().unwrap().credential_ref,
            "existing-ref"
        );
        server.await.unwrap();
        db.close().await;
    }
}
