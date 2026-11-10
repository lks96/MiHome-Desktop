use super::catalog::{CatalogBuilder, DeviceCatalog, safe_attributes};
use super::ha::{normalize_url, protocol_error};
use crate::{database::Database, errors::AppError, security::CredentialStore};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::Arc, time::Duration};
use tokio::{
    net::TcpStream,
    sync::{Mutex, RwLock},
    task::JoinHandle,
    time::{MissedTickBehavior, interval, timeout},
};
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream,
    tungstenite::{Message, protocol::WebSocketConfig},
};
use zeroize::Zeroizing;

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;
#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionState {
    Disconnected,
    Connecting,
    Authenticating,
    Connected,
    Reconnecting,
    Error,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionStatus {
    pub state: ConnectionState,
    pub attempt: u32,
    pub error: Option<AppError>,
}
impl Default for ConnectionStatus {
    fn default() -> Self {
        Self {
            state: ConnectionState::Disconnected,
            attempt: 0,
            error: None,
        }
    }
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntitySummary {
    pub entity_id: String,
    pub name: String,
    pub domain: String,
    pub state: String,
    pub unit: Option<String>,
    pub last_updated: String,
    pub attributes: Value,
}
#[derive(Deserialize)]
struct RawEntity {
    entity_id: String,
    state: String,
    #[serde(default)]
    attributes: Value,
    last_updated: String,
}
impl RawEntity {
    fn normalize(self) -> EntitySummary {
        EntitySummary {
            name: self
                .attributes
                .get("friendly_name")
                .and_then(Value::as_str)
                .unwrap_or(&self.entity_id)
                .to_owned(),
            domain: self.entity_id.split('.').next().unwrap_or("").to_owned(),
            unit: self
                .attributes
                .get("unit_of_measurement")
                .and_then(Value::as_str)
                .map(str::to_owned),
            attributes: safe_attributes(&self.attributes),
            entity_id: self.entity_id,
            state: self.state,
            last_updated: self.last_updated,
        }
    }
}
// Arbitrary HA attributes (including camera access_token) never cross IPC.
#[derive(Clone)]
pub enum HaEvent {
    Connection(ConnectionStatus),
    Synced(Vec<EntitySummary>),
    Changed(EntitySummary),
    Removed(String),
    SyncStarted,
    Catalog(DeviceCatalog),
}
type EventSink = Arc<dyn Fn(HaEvent) + Send + Sync>;
#[derive(Default)]
struct Snapshot {
    status: ConnectionStatus,
    entities: BTreeMap<String, EntitySummary>,
    catalog: DeviceCatalog,
    base_url: Option<String>,
}
#[derive(Clone)]
pub struct HaService {
    pub credentials: Arc<dyn CredentialStore>,
    pub operation: Arc<Mutex<()>>,
    worker: Arc<Mutex<Option<JoinHandle<()>>>>,
    snapshot: Arc<RwLock<Snapshot>>,
    sink: EventSink,
}
impl HaService {
    pub fn new(credentials: Arc<dyn CredentialStore>, sink: EventSink) -> Self {
        Self {
            credentials,
            operation: Arc::new(Mutex::new(())),
            worker: Arc::new(Mutex::new(None)),
            snapshot: Arc::new(RwLock::new(Snapshot::default())),
            sink,
        }
    }
    pub async fn status(&self) -> ConnectionStatus {
        self.snapshot.read().await.status.clone()
    }
    pub async fn entities(&self) -> Vec<EntitySummary> {
        self.snapshot
            .read()
            .await
            .entities
            .values()
            .cloned()
            .collect()
    }
    pub async fn bound_to(&self, base_url: &str) -> bool {
        self.snapshot.read().await.base_url.as_deref() == Some(base_url)
    }
    pub async fn catalog(&self) -> DeviceCatalog {
        self.snapshot.read().await.catalog.clone()
    }
    pub(crate) async fn set_catalog(&self, catalog: DeviceCatalog) {
        self.snapshot.write().await.catalog = catalog.clone();
        (self.sink)(HaEvent::Catalog(catalog));
    }
    async fn status_changed(&self, state: ConnectionState, attempt: u32, error: Option<AppError>) {
        let status = ConnectionStatus {
            state,
            attempt,
            error,
        };
        self.snapshot.write().await.status = status.clone();
        (self.sink)(HaEvent::Connection(status));
    }
    async fn stop_worker(&self) {
        if let Some(task) = self.worker.lock().await.take() {
            task.abort();
            let _ = task.await;
        }
    }
    pub async fn report_error(&self, error: AppError) {
        self.status_changed(ConnectionState::Error, 0, Some(error))
            .await;
    }
    pub async fn disconnect(&self) {
        let _operation = self.operation.lock().await;
        self.stop_worker().await;
        self.snapshot.write().await.entities.clear();
        self.set_catalog(DeviceCatalog::default()).await;
        (self.sink)(HaEvent::Synced(vec![]));
        self.snapshot.write().await.base_url = None;
        self.status_changed(ConnectionState::Disconnected, 0, None)
            .await;
    }
    pub async fn connect(&self, database: &Database) -> Result<ConnectionStatus, AppError> {
        let _operation = self.operation.lock().await;
        let connection = database
            .connection()
            .await?
            .ok_or_else(|| AppError::invalid_input("请先保存 Home Assistant 连接"))?;
        let settings = database.settings().await?;
        let token = self.credentials.read(&connection.credential_ref)?;
        let base_url = normalize_url(&connection.base_url)?;
        self.stop_worker().await;
        self.snapshot.write().await.entities.clear();
        self.set_catalog(DeviceCatalog::default()).await;
        (self.sink)(HaEvent::Synced(vec![]));
        self.snapshot.write().await.base_url = Some(base_url.to_string());
        self.status_changed(ConnectionState::Connecting, 0, None)
            .await;
        let service = self.clone();
        let worker = tokio::spawn(async move {
            let mut failures: u32 = 0;
            loop {
                let started = tokio::time::Instant::now();
                let result = service
                    .session(base_url.clone(), &token, settings.request_timeout)
                    .await;
                let error = result.err().unwrap_or_else(disconnected_error);
                // A stable authenticated session resets the retry delay. Invalid tokens require user action.
                if service.status().await.state == ConnectionState::Connected
                    && started.elapsed() >= Duration::from_secs(60)
                {
                    failures = 0;
                }
                if !settings.auto_reconnect || !error.retryable {
                    service
                        .status_changed(ConnectionState::Error, failures, Some(error))
                        .await;
                    break;
                }
                failures = failures.saturating_add(1);
                service
                    .status_changed(ConnectionState::Reconnecting, failures, Some(error))
                    .await;
                tokio::time::sleep(retry_delay(failures)).await;
                service
                    .status_changed(ConnectionState::Connecting, failures, None)
                    .await;
            }
        });
        *self.worker.lock().await = Some(worker);
        Ok(self.status().await)
    }
    async fn session(
        &self,
        base_url: url::Url,
        token: &Zeroizing<String>,
        seconds: u32,
    ) -> Result<(), AppError> {
        let mut url = base_url
            .join("api/websocket")
            .map_err(|_| protocol_error())?;
        let scheme = if url.scheme() == "https" { "wss" } else { "ws" };
        url.set_scheme(scheme).map_err(|_| protocol_error())?;
        let wait = Duration::from_secs(seconds as u64);
        let config = WebSocketConfig::default()
            .max_message_size(Some(16 * 1024 * 1024))
            .max_frame_size(Some(16 * 1024 * 1024));
        let (mut socket, _) = timeout(
            wait,
            tokio_tungstenite::connect_async_with_config(url.as_str(), Some(config), false),
        )
        .await
        .map_err(|_| disconnected_error())?
        .map_err(|_| disconnected_error())?;
        if receive(&mut socket, wait).await?["type"] != "auth_required" {
            return Err(protocol_error());
        }
        self.status_changed(ConnectionState::Authenticating, 0, None)
            .await;
        send(
            &mut socket,
            json!({"type":"auth", "access_token":token.as_str()}),
            wait,
        )
        .await?;
        let auth = receive(&mut socket, wait).await?;
        if auth["type"] == "auth_invalid" {
            return Err(AppError::new(
                "HA_AUTH_INVALID",
                "WebSocket 认证失败，请更新访问令牌",
                false,
            ));
        }
        if auth["type"] != "auth_ok" {
            return Err(protocol_error());
        }
        (self.sink)(HaEvent::SyncStarted);
        send(
            &mut socket,
            json!({"id":1,"type":"subscribe_events","event_type":"state_changed"}),
            wait,
        )
        .await?;
        let response = receive(&mut socket, wait).await?;
        if response["id"] != 1 || response["success"] != true {
            return Err(protocol_error());
        }
        send(&mut socket, json!({"id":2,"type":"get_states"}), wait).await?;
        let mut buffered = Vec::new();
        // One total synchronization deadline even if the remote sends unrelated messages.
        let states = timeout(wait, async {
            loop {
                let response = receive(&mut socket, wait).await?;
                if response["type"] == "event" && response["id"] == 1 {
                    if buffered.len() >= 10000 {
                        return Err(protocol_error());
                    }
                    buffered.push(response);
                } else if response["id"] == 2 {
                    if response["success"] != true {
                        return Err(protocol_error());
                    }
                    return serde_json::from_value::<Vec<RawEntity>>(response["result"].clone())
                        .map_err(|_| protocol_error());
                }
            }
        })
        .await
        .map_err(|_| disconnected_error())??;
        {
            let mut snapshot = self.snapshot.write().await;
            snapshot.entities = states
                .into_iter()
                .map(RawEntity::normalize)
                .map(|entity| (entity.entity_id.clone(), entity))
                .collect();
        }
        for event in buffered {
            self.apply_event(event).await?;
        }
        (self.sink)(HaEvent::Synced(self.entities().await));
        self.status_changed(ConnectionState::Connected, 0, None)
            .await;
        // Optional metadata requests must not gate the basic entity snapshot.
        self.set_catalog(DeviceCatalog::default()).await;
        let mut catalog = CatalogBuilder::default();
        for (id, kind) in [
            (10, "get_services"),
            (11, "config/area_registry/list"),
            (12, "config/entity_registry/list"),
            (13, "config/device_registry/list"),
        ] {
            send(&mut socket, json!({"id":id,"type":kind}), wait).await?;
        }
        let mut heartbeat = interval(Duration::from_secs(25));
        heartbeat.set_missed_tick_behavior(MissedTickBehavior::Skip);
        heartbeat.tick().await;
        let mut pending_ping: Option<u64> = None;
        let mut id = 100_u64;
        loop {
            tokio::select! {
                next = socket.next() => {
                    match next {
                        Some(Ok(Message::Text(text))) => {
                            let message: Value = serde_json::from_str(&text).map_err(|_| protocol_error())?;
                            if message["type"] == "pong" && message["id"].as_u64() == pending_ping { pending_ping = None; }
                            else if message["type"] == "event" && message["id"] == 1 { self.apply_event(message).await?; }
                            else if message["type"] == "result" { if let Some(value) = catalog.apply(&message) { self.set_catalog(value).await; } }
                        }
                        Some(Ok(Message::Ping(data))) => { timeout(wait, socket.send(Message::Pong(data))).await.map_err(|_| disconnected_error())?.map_err(|_| disconnected_error())?; }
                        Some(Ok(Message::Pong(_))) => {},
                        _ => return Err(disconnected_error()),
                    }
                }
                _ = heartbeat.tick() => {
                    if pending_ping.is_some() { return Err(disconnected_error()); }
                    send(&mut socket, json!({"id":id,"type":"ping"}), wait).await?;
                    pending_ping = Some(id);
                    id += 1;
                }
            }
        }
    }
    async fn apply_event(&self, message: Value) -> Result<(), AppError> {
        let data = &message["event"]["data"];
        let entity_id = data["entity_id"]
            .as_str()
            .ok_or_else(protocol_error)?
            .to_owned();
        let mut snapshot = self.snapshot.write().await;
        let timestamp = data["new_state"]["last_updated"]
            .as_str()
            .or_else(|| message["event"]["time_fired"].as_str());
        if let (Some(previous), Some(timestamp)) = (snapshot.entities.get(&entity_id), timestamp) {
            if let (Ok(before), Ok(after)) = (
                chrono::DateTime::parse_from_rfc3339(&previous.last_updated),
                chrono::DateTime::parse_from_rfc3339(timestamp),
            ) {
                if after < before {
                    return Ok(());
                }
            }
        }
        let event = if data.get("new_state") == Some(&Value::Null) {
            snapshot.entities.remove(&entity_id);
            HaEvent::Removed(entity_id)
        } else {
            let entity: RawEntity =
                serde_json::from_value(data["new_state"].clone()).map_err(|_| protocol_error())?;
            if entity.entity_id != entity_id {
                return Err(protocol_error());
            }
            let entity = entity.normalize();
            snapshot.entities.insert(entity_id, entity.clone());
            HaEvent::Changed(entity)
        };
        drop(snapshot);
        (self.sink)(event);
        Ok(())
    }
}
fn disconnected_error() -> AppError {
    AppError::new(
        "HA_WS_DISCONNECTED",
        "Home Assistant 实时连接已中断或超时",
        true,
    )
}
async fn receive(socket: &mut Socket, wait: Duration) -> Result<Value, AppError> {
    timeout(wait, async {
        loop {
            match socket.next().await {
                Some(Ok(Message::Text(text))) => {
                    return serde_json::from_str(&text).map_err(|_| protocol_error());
                }
                Some(Ok(Message::Ping(data))) => {
                    socket
                        .send(Message::Pong(data))
                        .await
                        .map_err(|_| disconnected_error())?;
                }
                Some(Ok(Message::Pong(_))) => {}
                _ => return Err(disconnected_error()),
            }
        }
    })
    .await
    .map_err(|_| disconnected_error())?
}
async fn send(socket: &mut Socket, value: Value, wait: Duration) -> Result<(), AppError> {
    timeout(wait, socket.send(Message::Text(value.to_string().into())))
        .await
        .map_err(|_| disconnected_error())?
        .map_err(|_| disconnected_error())
}
fn retry_delay(attempt: u32) -> Duration {
    let seconds = match attempt {
        0 | 1 => 1,
        2 => 2,
        3 => 5,
        4 => 10,
        _ => 29,
    };
    let jitter =
        u16::from_le_bytes(uuid::Uuid::new_v4().as_bytes()[..2].try_into().unwrap()) % 1000;
    Duration::from_millis(seconds * 1000 + u64::from(jitter))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::security::MemoryCredentials;
    use tokio::{net::TcpListener, sync::mpsc};
    async fn next_json<S>(ws: &mut WebSocketStream<S>) -> Value
    where
        S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
    {
        let message = ws.next().await.unwrap().unwrap();
        serde_json::from_str(message.to_text().unwrap()).unwrap()
    }
    #[tokio::test]
    async fn authenticates_syncs_reconnects_and_cancels_without_leaking_attributes() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/", listener.local_addr().unwrap());
        let fixture = tokio::spawn(async move {
            for attempt in 0..2 {
                let (stream, _) = listener.accept().await.unwrap();
                let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();
                ws.send(Message::Text(
                    json!({"type":"auth_required"}).to_string().into(),
                ))
                .await
                .unwrap();
                let auth = next_json(&mut ws).await;
                assert_eq!(auth["access_token"], "test-only-token");
                ws.send(Message::Text(json!({"type":"auth_ok"}).to_string().into()))
                    .await
                    .unwrap();
                assert_eq!(next_json(&mut ws).await["event_type"], "state_changed");
                ws.send(Message::Text(
                    json!({"id":1,"type":"result","success":true,"result":null})
                        .to_string()
                        .into(),
                ))
                .await
                .unwrap();
                assert_eq!(next_json(&mut ws).await["type"], "get_states");
                let entity = json!({"entity_id":"light.office","state":"off","last_updated":"2026-11-10T01:00:00Z","attributes":{"friendly_name":"Office", "access_token":"must-not-cross-ipc"}});
                ws.send(Message::Text(
                    json!({"id":2,"type":"result","success":true,"result":[entity]})
                        .to_string()
                        .into(),
                ))
                .await
                .unwrap();
                if attempt == 0 {
                    ws.close(None).await.unwrap();
                } else {
                    let mut changed = entity.clone();
                    changed["state"] = json!("on");
                    changed["last_updated"] = json!("2026-11-10T01:00:01Z");
                    ws.send(Message::Text(json!({"id":1,"type":"event","event":{"time_fired":"2026-11-10T01:00:01Z","data":{"entity_id":"light.office","new_state":changed}}}).to_string().into())).await.unwrap();
                    // Stay alive until explicit client disconnect cancels the worker.
                    while let Some(Ok(Message::Text(text))) = ws.next().await {
                        let request: Value = serde_json::from_str(&text).unwrap();
                        let result = if request["id"] == 10 {
                            json!({"light":{"turn_on":{},"turn_off":{}}})
                        } else {
                            json!([])
                        };
                        ws.send(Message::Text(json!({"id":request["id"],"type":"result","success":true,"result":result}).to_string().into())).await.unwrap();
                    }
                }
            }
        });
        let dir = tempfile::tempdir().unwrap();
        let db = Database::open(&dir.path().join("session.sqlite"))
            .await
            .unwrap();
        sqlx::query("INSERT INTO ha_connections(id, name, base_url, credential_ref) VALUES('primary', 'Home', ?, 'test-ref')").bind(&url).execute(&db.pool).await.unwrap();
        let store = Arc::new(MemoryCredentials::default());
        store.write("test-ref", "test-only-token").unwrap();
        let (tx, mut rx) = mpsc::unbounded_channel();
        let service = HaService::new(
            store,
            Arc::new(move |event| {
                let _ = tx.send(event);
            }),
        );
        service.connect(&db).await.unwrap();
        timeout(Duration::from_secs(8), async {
            let mut connected = 0;
            let mut reconnecting = false;
            while let Some(event) = rx.recv().await {
                match event {
                    HaEvent::Connection(status) if status.state == ConnectionState::Connected => {
                        connected += 1
                    }
                    HaEvent::Connection(status)
                        if status.state == ConnectionState::Reconnecting =>
                    {
                        reconnecting = true
                    }
                    HaEvent::Changed(entity) if entity.state == "on" => {
                        assert_eq!(connected, 2);
                        assert!(reconnecting);
                        assert!(
                            !serde_json::to_string(&entity)
                                .unwrap()
                                .contains("must-not-cross-ipc")
                        );
                        break;
                    }
                    _ => {}
                }
            }
        })
        .await
        .unwrap();
        assert_eq!(service.entities().await[0].state, "on");
        service.disconnect().await;
        assert_eq!(service.status().await.state, ConnectionState::Disconnected);
        assert!(service.entities().await.is_empty());
        timeout(Duration::from_secs(2), fixture)
            .await
            .unwrap()
            .unwrap();
        db.close().await;
    }
    #[tokio::test]
    async fn scene_execution_posts_one_target_and_waits_for_state_events() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base_url = format!("http://{}/", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            loop {
                let mut buffer = [0; 4096];
                let count = socket.read(&mut buffer).await.unwrap();
                assert!(count > 0);
                request.extend_from_slice(&buffer[..count]);
                if let Some(end) = request.windows(4).position(|value| value == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&request[..end]).to_ascii_lowercase();
                    let length: usize = headers
                        .lines()
                        .find_map(|line| line.strip_prefix("content-length: "))
                        .unwrap()
                        .parse()
                        .unwrap();
                    if request.len() >= end + 4 + length {
                        assert!(headers.starts_with("post /api/services/scene/turn_on http/1.1"));
                        assert!(headers.contains("authorization: bearer test-only-token"));
                        let body: Value =
                            serde_json::from_slice(&request[end + 4..end + 4 + length]).unwrap();
                        assert_eq!(body, json!({"entity_id":"scene.evening"}));
                        break;
                    }
                }
            }
            socket
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n[]")
                .await
                .unwrap();
        });
        let dir = tempfile::tempdir().unwrap();
        let db = Database::open(&dir.path().join("scene.sqlite"))
            .await
            .unwrap();
        sqlx::query("INSERT INTO ha_connections(id,name,base_url,credential_ref) VALUES('primary','Home',?,'test-ref')").bind(&base_url).execute(&db.pool).await.unwrap();
        let credentials = Arc::new(MemoryCredentials::default());
        credentials.write("test-ref", "test-only-token").unwrap();
        let service = HaService::new(credentials, Arc::new(|_| {}));
        service.snapshot.write().await.base_url = Some(base_url);
        service
            .set_catalog(DeviceCatalog {
                services: std::collections::BTreeMap::from([(
                    "scene".into(),
                    vec!["turn_on".into()],
                )]),
                services_loaded: true,
                ..Default::default()
            })
            .await;
        service
            .status_changed(ConnectionState::Connected, 0, None)
            .await;
        service.apply_event(json!({"event":{"data":{"entity_id":"scene.evening","new_state":{"entity_id":"scene.evening","state":"unknown","last_updated":"2026-11-10T01:00:00Z"}}}})).await.unwrap();
        assert!(
            service
                .execute_scene(&db, "scene.evening")
                .await
                .unwrap()
                .accepted
        );
        assert_eq!(service.entities().await[0].state, "unknown");
        server.await.unwrap();
        db.close().await;
    }

    #[tokio::test]
    async fn snapshots_use_backend_token_and_ha_camera_proxy_route() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base_url = format!("http://{}/ha/", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            loop {
                let mut bytes = [0; 4096];
                let count = socket.read(&mut bytes).await.unwrap();
                assert!(count > 0);
                request.extend_from_slice(&bytes[..count]);
                if request.windows(4).any(|value| value == b"\r\n\r\n") {
                    break;
                }
            }
            let headers = String::from_utf8_lossy(&request).to_ascii_lowercase();
            assert!(headers.starts_with("get /ha/api/camera_proxy/camera.office http/1.1"));
            assert!(headers.contains("authorization: bearer test-only-token"));
            socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: image/jpeg\r\nContent-Length: 4\r\nConnection: close\r\n\r\n\xff\xd8\xff\x00").await.unwrap();
        });
        let dir = tempfile::tempdir().unwrap();
        let db = Database::open(&dir.path().join("snapshot.sqlite"))
            .await
            .unwrap();
        sqlx::query("INSERT INTO ha_connections(id,name,base_url,credential_ref) VALUES('primary','Home',?,'test-ref')").bind(&base_url).execute(&db.pool).await.unwrap();
        let credentials = Arc::new(MemoryCredentials::default());
        credentials.write("test-ref", "test-only-token").unwrap();
        let service = HaService::new(credentials, Arc::new(|_| {}));
        service.snapshot.write().await.base_url = Some(base_url);
        service
            .status_changed(ConnectionState::Connected, 0, None)
            .await;
        service.apply_event(json!({"event":{"data":{"entity_id":"camera.office","new_state":{"entity_id":"camera.office","state":"idle","last_updated":"2026-11-10T01:00:00Z"}}}})).await.unwrap();
        let result = service.camera_snapshot(&db, "camera.office").await.unwrap();
        assert!(result.data_url.starts_with("data:image/jpeg;base64,"));
        assert!(!result.data_url.contains("test-only-token"));
        server.await.unwrap();
        db.close().await;
    }

    #[tokio::test]
    async fn invalid_websocket_token_is_not_retryable() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();
            ws.send(Message::Text(
                json!({"type":"auth_required"}).to_string().into(),
            ))
            .await
            .unwrap();
            assert_eq!(next_json(&mut ws).await["type"], "auth");
            ws.send(Message::Text(
                json!({"type":"auth_invalid","message":"sensitive error from upstream"})
                    .to_string()
                    .into(),
            ))
            .await
            .unwrap();
        });
        let service = HaService::new(Arc::new(MemoryCredentials::default()), Arc::new(|_| {}));
        let result = service
            .session(
                normalize_url(&url).unwrap(),
                &Zeroizing::new("test-only-token".into()),
                2,
            )
            .await
            .unwrap_err();
        assert_eq!(result.code, "HA_AUTH_INVALID");
        assert!(!result.retryable);
        assert!(!result.message.contains("sensitive error"));
        server.await.unwrap();
    }

    #[tokio::test]
    async fn removes_entities_and_ignores_older_events() {
        let service = HaService::new(Arc::new(MemoryCredentials::default()), Arc::new(|_| {}));
        let event = |state: &str, time: &str| json!({"event":{"data":{"entity_id":"switch.test","new_state":{"entity_id":"switch.test","state":state,"last_updated":time}}}});
        service
            .apply_event(event("on", "2026-11-10T01:00:02Z"))
            .await
            .unwrap();
        service
            .apply_event(event("off", "2026-11-10T01:00:01Z"))
            .await
            .unwrap();
        assert_eq!(service.entities().await[0].state, "on");
        service.apply_event(json!({"event":{"time_fired":"2026-11-10T01:00:03Z","data":{"entity_id":"switch.test","new_state":null}}})).await.unwrap();
        assert!(service.entities().await.is_empty());
    }
}
