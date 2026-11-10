use super::{
    devices::validate_entity_id,
    ha::normalize_url,
    session::{ConnectionState, HaService},
};
use crate::{database::Database, errors::AppError};
use base64::{Engine, engine::general_purpose::STANDARD};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};
use tokio::sync::Mutex;

#[derive(Clone, Default, Debug, Serialize, Deserialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Gateway {
    pub base_url: String,
    pub enabled: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CameraProfile {
    pub entity_id: String,
    pub source_type: String,
    pub stream_name: Option<String>,
    pub enabled: bool,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Probe {
    pub stream_names: Vec<String>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub data_url: String,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CameraSession {
    pub session_id: String,
    pub entity_id: String,
}
struct Lease {
    entity_id: String,
    base_url: String,
    stream_name: String,
    expires: Instant,
    used: bool,
}
#[derive(Default)]
pub struct CameraSessions {
    leases: Mutex<HashMap<String, Lease>>,
}

pub fn camera_id(id: &str) -> Result<(), AppError> {
    validate_entity_id(id)?;
    if !id.starts_with("camera.") {
        return Err(AppError::invalid_input("必须选择 camera 实体"));
    }
    Ok(())
}
fn valid_alias(alias: &str) -> bool {
    !alias.is_empty()
        && alias.len() <= 128
        && alias
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
}
fn error(code: &'static str, message: &str) -> AppError {
    AppError::new(code, message, true)
}
fn unavailable() -> AppError {
    error(
        "GO2RTC_UNREACHABLE",
        "无法访问 go2rtc，请检查地址、网络及服务配置",
    )
}
fn no_session() -> AppError {
    error("CAMERA_SESSION_EXPIRED", "播放会话已关闭或过期，请重新连接")
}
fn client(seconds: u32) -> Result<reqwest::Client, AppError> {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(seconds.clamp(1, 120) as u64))
        .build()
        .map_err(|_| unavailable())
}
async fn bounded_body(response: reqwest::Response, max: usize) -> Result<Vec<u8>, AppError> {
    let mut bytes = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| unavailable())?;
        if bytes.len() + chunk.len() > max {
            return Err(error("CAMERA_RESPONSE_INVALID", "视频服务响应超过限制"));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}
fn check_status(status: reqwest::StatusCode) -> Result<(), AppError> {
    if status == 401 || status == 403 {
        return Err(AppError::new(
            "GO2RTC_AUTH_REQUIRED",
            "go2rtc API 要求认证，当前客户端尚未配置该认证方式",
            false,
        ));
    }
    if status == 404 {
        return Err(error(
            "CAMERA_NO_STREAM",
            "未找到指定视频流，请核对 go2rtc 流别名",
        ));
    }
    if !status.is_success() {
        return Err(unavailable());
    }
    Ok(())
}
pub async fn probe_gateway(gateway: &Gateway, seconds: u32) -> Result<Probe, AppError> {
    let endpoint = normalize_url(&gateway.base_url)?
        .join("api/streams")
        .map_err(|_| unavailable())?;
    let response = client(seconds)?
        .get(endpoint)
        .send()
        .await
        .map_err(|_| unavailable())?;
    check_status(response.status())?;
    let body = bounded_body(response, 2 * 1024 * 1024).await?;
    let streams: serde_json::Value = serde_json::from_slice(&body).map_err(|_| unavailable())?;
    let names = streams
        .as_object()
        .ok_or_else(unavailable)?
        .keys()
        .filter(|name| valid_alias(name))
        .cloned()
        .collect();
    // Values can contain camera passwords and source URLs: return names only.
    Ok(Probe {
        stream_names: names,
    })
}
impl Database {
    pub async fn camera_gateway(&self) -> Result<Gateway, AppError> {
        sqlx::query_as("SELECT base_url, enabled FROM camera_gateway WHERE id=1")
            .fetch_optional(&self.pool)
            .await
            .map(|value| value.unwrap_or_default())
            .map_err(|_| AppError::database("load_camera_gateway"))
    }
    pub async fn save_camera_gateway(&self, mut value: Gateway) -> Result<Gateway, AppError> {
        if !value.base_url.trim().is_empty() {
            value.base_url = normalize_url(&value.base_url)?.to_string();
        } else if value.enabled {
            return Err(AppError::invalid_input("启用 go2rtc 前请填写地址"));
        }
        sqlx::query("INSERT INTO camera_gateway(id,base_url,enabled) VALUES(1,?,?) ON CONFLICT(id) DO UPDATE SET base_url=excluded.base_url,enabled=excluded.enabled")
            .bind(&value.base_url).bind(value.enabled).execute(&self.pool).await.map_err(|_|AppError::database("save_camera_gateway"))?;
        Ok(value)
    }
    pub async fn camera_profiles(&self) -> Result<Vec<CameraProfile>, AppError> {
        sqlx::query_as("SELECT entity_id,source_type,stream_name,enabled FROM camera_profiles ORDER BY entity_id").fetch_all(&self.pool).await.map_err(|_|AppError::database("load_camera_profiles"))
    }
    pub async fn save_camera_profile(
        &self,
        mut profile: CameraProfile,
    ) -> Result<CameraProfile, AppError> {
        camera_id(&profile.entity_id)?;
        match profile.source_type.as_str() {
            "go2rtc" => {
                if !profile.stream_name.as_deref().is_some_and(valid_alias) {
                    return Err(AppError::invalid_input(
                        "流别名仅允许字母、数字、下划线、点和连字符（最多128字符），不要填写 RTSP 或认证 URL",
                    ));
                }
            }
            "snapshot" => {
                profile.stream_name = None;
            }
            _ => return Err(AppError::invalid_input("不支持此视频源类型")),
        }
        sqlx::query("INSERT INTO camera_profiles(entity_id,source_type,stream_name,enabled) VALUES(?,?,?,?) ON CONFLICT(entity_id) DO UPDATE SET source_type=excluded.source_type,stream_name=excluded.stream_name,enabled=excluded.enabled,updated_at=datetime('now')")
            .bind(&profile.entity_id).bind(&profile.source_type).bind(&profile.stream_name).bind(profile.enabled).execute(&self.pool).await.map_err(|_|AppError::database("save_camera_profile"))?;
        Ok(profile)
    }
}
impl CameraSessions {
    pub async fn clear(&self) {
        self.leases.lock().await.clear();
    }
    pub async fn start(
        &self,
        database: &Database,
        entity_id: &str,
    ) -> Result<CameraSession, AppError> {
        camera_id(entity_id)?;
        let profile = database
            .camera_profiles()
            .await?
            .into_iter()
            .find(|profile| {
                profile.entity_id == entity_id && profile.enabled && profile.source_type == "go2rtc"
            })
            .ok_or_else(|| error("CAMERA_NO_STREAM", "请先为摄像头启用 go2rtc 配置"))?;
        let gateway = database.camera_gateway().await?;
        if !gateway.enabled {
            return Err(error("GO2RTC_UNREACHABLE", "请先在设置中启用 go2rtc"));
        }
        let base_url = normalize_url(&gateway.base_url)?.to_string();
        let alias = profile
            .stream_name
            .filter(|name| valid_alias(name))
            .ok_or_else(|| error("CAMERA_NO_STREAM", "流别名无效"))?;
        let mut leases = self.leases.lock().await;
        leases.retain(|_, lease| lease.expires > Instant::now());
        if leases.values().any(|lease| lease.entity_id == entity_id) {
            return Err(error(
                "CAMERA_SESSION_BUSY",
                "此摄像头已有播放会话，请先关闭后重试",
            ));
        }
        if leases.len() >= 4 {
            return Err(error("CAMERA_SESSION_LIMIT", "最多同时播放四路视频"));
        }
        let id = uuid::Uuid::new_v4().to_string();
        leases.insert(
            id.clone(),
            Lease {
                entity_id: entity_id.into(),
                base_url,
                stream_name: alias,
                expires: Instant::now() + Duration::from_secs(45),
                used: false,
            },
        );
        Ok(CameraSession {
            session_id: id,
            entity_id: entity_id.into(),
        })
    }
    pub async fn keep(&self, id: &str) -> Result<bool, AppError> {
        let mut leases = self.leases.lock().await;
        let lease = leases
            .get_mut(id)
            .filter(|lease| lease.expires > Instant::now())
            .ok_or_else(no_session)?;
        lease.expires = Instant::now() + Duration::from_secs(45);
        Ok(true)
    }
    pub async fn stop(&self, id: &str) -> bool {
        self.leases.lock().await.remove(id).is_some()
    }
    pub async fn exchange(&self, id: &str, offer: &str) -> Result<String, AppError> {
        if offer.len() > 256 * 1024 || !offer.starts_with("v=0") || !offer.contains("m=video ") {
            return Err(AppError::invalid_input("WebRTC 协商数据格式无效或过大"));
        }
        let (base_url, alias) = {
            let mut leases = self.leases.lock().await;
            let lease = leases
                .get_mut(id)
                .filter(|lease| lease.expires > Instant::now() && !lease.used)
                .ok_or_else(no_session)?;
            lease.used = true;
            (lease.base_url.clone(), lease.stream_name.clone())
        };
        let result = async {
            let mut endpoint = normalize_url(&base_url)?
                .join("api/webrtc")
                .map_err(|_| unavailable())?;
            endpoint.query_pairs_mut().append_pair("src", &alias);
            let response = client(15)?
                .post(endpoint)
                .header("Content-Type", "application/sdp")
                .body(offer.to_owned())
                .send()
                .await
                .map_err(|_| unavailable())?;
            check_status(response.status())?;
            let body = bounded_body(response, 256 * 1024).await?;
            let answer = String::from_utf8(body)
                .map_err(|_| error("CAMERA_RESPONSE_INVALID", "视频协商响应无效"))?;
            if !answer.starts_with("v=0") || !answer.contains("m=video ") {
                return Err(error("CAMERA_RESPONSE_INVALID", "视频服务未返回有效 SDP"));
            }
            self.keep(id).await?;
            Ok(answer)
        }
        .await;
        if result.is_err() {
            self.stop(id).await;
        }
        result
    }
}
impl HaService {
    pub async fn camera_snapshot(
        &self,
        database: &Database,
        entity_id: &str,
    ) -> Result<Snapshot, AppError> {
        camera_id(entity_id)?;
        let _operation = self.operation.lock().await;
        if self.status().await.state != ConnectionState::Connected {
            return Err(error(
                "HA_WS_DISCONNECTED",
                "请先连接 Home Assistant 再获取截图",
            ));
        }
        let entity = self
            .entities()
            .await
            .into_iter()
            .find(|entity| entity.entity_id == entity_id)
            .ok_or_else(|| error("ENTITY_NOT_FOUND", "未找到摄像头实体"))?;
        if ["unavailable", "unknown"].contains(&entity.state.as_str()) {
            return Err(error("CAMERA_OFFLINE", "HA 标记摄像头为离线或状态未知"));
        }
        let connection = database
            .connection()
            .await?
            .ok_or_else(|| error("HA_UNREACHABLE", "未配置 HA"))?;
        if !self.bound_to(&connection.base_url).await {
            return Err(error("HA_SYNC_FAILED", "连接配置已变化，请重新同步"));
        }
        let token = self.credentials.read(&connection.credential_ref)?;
        let endpoint = normalize_url(&connection.base_url)?
            .join(&format!("api/camera_proxy/{entity_id}"))
            .map_err(|_| unavailable())?;
        let response = client(database.settings().await?.request_timeout)?
            .get(endpoint)
            .bearer_auth(token.as_str())
            .send()
            .await
            .map_err(|_| error("CAMERA_TIMEOUT", "获取摄像头截图超时或网络不可用"))?;
        if response.status() == 401 || response.status() == 403 {
            return Err(AppError::new(
                "HA_AUTH_INVALID",
                "没有访问摄像头截图的权限",
                false,
            ));
        }
        if !response.status().is_success() {
            return Err(error("CAMERA_UNSUPPORTED", "HA 未能提供摄像头截图"));
        }
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .unwrap_or("")
            .split(';')
            .next()
            .unwrap_or("")
            .to_owned();
        let bytes = bounded_body(response, 8 * 1024 * 1024).await?;
        snapshot_image(&content_type, &bytes)
    }
}
fn snapshot_image(content_type: &str, bytes: &[u8]) -> Result<Snapshot, AppError> {
    let valid = match content_type {
        "image/jpeg" => bytes.starts_with(&[0xff, 0xd8, 0xff]),
        "image/png" => bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
        "image/webp" => bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP"),
        _ => false,
    };
    if !valid {
        return Err(error(
            "CAMERA_RESPONSE_INVALID",
            "截图不是支持的 JPEG、PNG 或 WebP 图片",
        ));
    }
    Ok(Snapshot {
        data_url: format!("data:{content_type};base64,{}", STANDARD.encode(bytes)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::Arc;
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };
    const SDP: &str = "v=0\r\nm=video 9 UDP/TLS/RTP/SAVPF 96\r\n";
    async fn database() -> (tempfile::TempDir, Database) {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::open(&dir.path().join("cameras.sqlite"))
            .await
            .unwrap();
        (dir, db)
    }
    fn profile(id: &str) -> CameraProfile {
        CameraProfile {
            entity_id: id.into(),
            source_type: "go2rtc".into(),
            stream_name: Some("office_cam".into()),
            enabled: true,
        }
    }
    #[tokio::test]
    async fn configuration_persists_and_rejects_credentials_or_source_urls() {
        let (dir, db) = database().await;
        for url in [
            "http://user:password@localhost:1984",
            "http://localhost:1984/?token=secret",
        ] {
            assert!(
                db.save_camera_gateway(Gateway {
                    base_url: url.into(),
                    enabled: true
                })
                .await
                .is_err()
            );
        }
        db.save_camera_gateway(Gateway {
            base_url: "http://localhost:1984".into(),
            enabled: true,
        })
        .await
        .unwrap();
        let mut invalid = profile("camera.office");
        invalid.stream_name = Some("rtsp://user:password@camera/stream".into());
        assert!(db.save_camera_profile(invalid).await.is_err());
        assert!(
            db.save_camera_profile(profile("switch.office"))
                .await
                .is_err()
        );
        db.save_camera_profile(profile("camera.office"))
            .await
            .unwrap();
        db.close().await;
        let reopened = Database::open(&dir.path().join("cameras.sqlite"))
            .await
            .unwrap();
        assert!(reopened.camera_gateway().await.unwrap().enabled);
        assert_eq!(
            reopened.camera_profiles().await.unwrap()[0]
                .stream_name
                .as_deref(),
            Some("office_cam")
        );
        reopened.close().await;
    }
    #[tokio::test]
    async fn duplicate_limit_expiration_and_stop_prevent_session_leaks() {
        let (_dir, db) = database().await;
        db.save_camera_gateway(Gateway {
            base_url: "http://localhost:1984".into(),
            enabled: true,
        })
        .await
        .unwrap();
        for index in 0..5 {
            db.save_camera_profile(profile(&format!("camera.test{index}")))
                .await
                .unwrap();
        }
        let sessions = CameraSessions::default();
        let mut ids = Vec::new();
        for index in 0..4 {
            ids.push(
                sessions
                    .start(&db, &format!("camera.test{index}"))
                    .await
                    .unwrap()
                    .session_id,
            );
        }
        assert_eq!(
            sessions
                .start(&db, "camera.test0")
                .await
                .err()
                .unwrap()
                .code,
            "CAMERA_SESSION_BUSY"
        );
        assert_eq!(
            sessions
                .start(&db, "camera.test4")
                .await
                .err()
                .unwrap()
                .code,
            "CAMERA_SESSION_LIMIT"
        );
        sessions
            .leases
            .lock()
            .await
            .get_mut(&ids[0])
            .unwrap()
            .expires = Instant::now() - Duration::from_secs(1);
        assert!(sessions.keep(&ids[0]).await.is_err());
        let fifth = sessions.start(&db, "camera.test4").await.unwrap();
        assert!(sessions.stop(&fifth.session_id).await);
        assert!(!sessions.stop(&fifth.session_id).await);
        sessions.clear().await;
        assert!(sessions.leases.lock().await.is_empty());
        db.close().await;
    }
    #[tokio::test]
    async fn stream_probe_only_returns_aliases_and_does_not_expose_source_secrets() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base_url = format!("http://{}/", listener.local_addr().unwrap());
        let fixture = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut input = [0; 4096];
            let length = socket.read(&mut input).await.unwrap();
            assert!(String::from_utf8_lossy(&input[..length]).starts_with("GET /api/streams "));
            let body=json!({"office_cam":{"producers":[{"url":"rtsp://user:password@camera"}]},"rtsp://secret:password@camera":{}}).to_string();
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
        let probe = probe_gateway(
            &Gateway {
                base_url,
                enabled: true,
            },
            2,
        )
        .await
        .unwrap();
        assert_eq!(probe.stream_names, vec!["office_cam"]);
        assert!(!serde_json::to_string(&probe).unwrap().contains("password"));
        fixture.await.unwrap();
    }
    #[tokio::test]
    async fn sdp_exchange_is_bounded_single_use_and_cancellation_discards_answer() {
        let (_dir, db) = database().await;
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base_url = format!("http://{}/", listener.local_addr().unwrap());
        db.save_camera_gateway(Gateway {
            base_url,
            enabled: true,
        })
        .await
        .unwrap();
        db.save_camera_profile(profile("camera.office"))
            .await
            .unwrap();
        let sessions = Arc::new(CameraSessions::default());
        let (tx, rx) = tokio::sync::oneshot::channel();
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        let fixture = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut bytes = Vec::new();
            loop {
                let mut chunk = [0; 4096];
                let count = socket.read(&mut chunk).await.unwrap();
                assert!(count > 0);
                bytes.extend_from_slice(&chunk[..count]);
                if let Some(end) = bytes.windows(4).position(|value| value == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&bytes[..end]).to_ascii_lowercase();
                    let length: usize = headers
                        .lines()
                        .find_map(|line| line.strip_prefix("content-length: "))
                        .unwrap()
                        .parse()
                        .unwrap();
                    if bytes.len() >= end + 4 + length {
                        assert!(headers.starts_with("post /api/webrtc?src=office_cam http/1.1"));
                        assert!(headers.contains("content-type: application/sdp"));
                        assert_eq!(&bytes[end + 4..end + 4 + length], SDP.as_bytes());
                        break;
                    }
                }
            }
            tx.send(()).unwrap();
            reply_rx.await.unwrap();
            socket.write_all(format!("HTTP/1.1 201 Created\r\nContent-Type: application/sdp\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{SDP}",SDP.len()).as_bytes()).await.unwrap();
        });
        let session = sessions.start(&db, "camera.office").await.unwrap();
        assert!(
            sessions
                .exchange(&session.session_id, "<html>not sdp</html>")
                .await
                .is_err()
        );
        let task = {
            let sessions = sessions.clone();
            let id = session.session_id.clone();
            tokio::spawn(async move { sessions.exchange(&id, SDP).await })
        };
        rx.await.unwrap();
        assert!(sessions.exchange(&session.session_id, SDP).await.is_err());
        sessions.stop(&session.session_id).await;
        reply_tx.send(()).unwrap();
        assert_eq!(
            task.await.unwrap().err().unwrap().code,
            "CAMERA_SESSION_EXPIRED"
        );
        fixture.await.unwrap();
        db.close().await;
    }
    #[tokio::test]
    async fn signaling_returns_only_valid_sdp_and_cleans_up_failed_sessions() {
        let (_dir, db) = database().await;
        db.save_camera_profile(profile("camera.office"))
            .await
            .unwrap();
        let sessions = CameraSessions::default();
        for (status, body, success) in [
            ("201 Created", SDP, true),
            ("200 OK", "<html>private upstream detail</html>", false),
            ("401 Unauthorized", "secret upstream error", false),
        ] {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            db.save_camera_gateway(Gateway {
                base_url: format!("http://{}/", listener.local_addr().unwrap()),
                enabled: true,
            })
            .await
            .unwrap();
            let server = tokio::spawn(async move {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut request = Vec::new();
                loop {
                    let mut bytes = [0; 4096];
                    let count = socket.read(&mut bytes).await.unwrap();
                    assert!(count > 0);
                    request.extend_from_slice(&bytes[..count]);
                    if let Some(end) = request.windows(4).position(|value| value == b"\r\n\r\n") {
                        let headers = String::from_utf8_lossy(&request[..end]).to_ascii_lowercase();
                        let length: usize = headers
                            .lines()
                            .find_map(|line| line.strip_prefix("content-length: "))
                            .unwrap()
                            .parse()
                            .unwrap();
                        if request.len() >= end + 4 + length {
                            break;
                        }
                    }
                }
                socket.write_all(format!("HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await.unwrap();
            });
            let session = sessions.start(&db, "camera.office").await.unwrap();
            let result = sessions.exchange(&session.session_id, SDP).await;
            if success {
                assert_eq!(result.unwrap(), SDP);
                sessions.stop(&session.session_id).await;
            } else {
                let error = result.err().unwrap();
                assert!(!error.message.contains("upstream"));
                assert!(sessions.keep(&session.session_id).await.is_err());
            }
            server.await.unwrap();
        }
        db.close().await;
    }

    #[test]
    fn snapshot_rejects_html_svg_and_mismatched_image_headers() {
        assert!(snapshot_image("image/svg+xml", b"<svg/>").is_err());
        assert!(snapshot_image("image/png", b"<html>secret</html>").is_err());
        assert!(snapshot_image("image/webp", b"RIFF1234notwebp").is_err());
        let image = snapshot_image("image/jpeg", &[0xff, 0xd8, 0xff, 0x00]).unwrap();
        assert!(image.data_url.starts_with("data:image/jpeg;base64,"));
    }
}
