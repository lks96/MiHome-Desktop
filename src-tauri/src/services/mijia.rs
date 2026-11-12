//! Xiaomi account QR authorization and direct MIoT cloud access.
//! No account responses, cookies, signed requests or credentials may be logged.
use crate::{errors::AppError, security::CredentialStore};
use base64::{Engine, engine::general_purpose::STANDARD as B64};
use futures_util::StreamExt;
use rand::RngCore;
use rc4::{
    Rc4,
    cipher::{KeyInit, StreamCipher, consts::U32},
};
use reqwest::{Client, Url};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha1::{Digest, Sha1};
use sha2::Sha256;
use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tokio::sync::Mutex;
use zeroize::{Zeroize, Zeroizing};

const SLOT: &str = "mijia.primary";
const AGENT: &str = "Android-7.1.1-1.0.0-ONEPLUS A3010-136-APP/xiaomi.smarthome APPV/10.0.0";
fn error(code: &'static str, message: &str, retryable: bool) -> AppError {
    AppError::new(code, message, retryable)
}
fn protocol() -> AppError {
    error(
        "MIJIA_PROTOCOL",
        "小米接口返回了无法识别的数据，请稍后重试或更新客户端",
        true,
    )
}
fn network() -> AppError {
    error(
        "MIJIA_NETWORK",
        "无法连接小米服务，请检查网络或稍后重试",
        true,
    )
}
fn expired() -> AppError {
    error("MIJIA_AUTH_EXPIRED", "米家登录已失效，请重新扫码", false)
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
fn region_valid(region: &str) -> bool {
    ["cn", "de", "us", "ru", "tw", "sg", "in", "i2"].contains(&region)
}
fn client() -> Result<Client, AppError> {
    Client::builder()
        .user_agent(AGENT)
        .cookie_store(true)
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|_| network())
}
async fn body(response: reqwest::Response, limit: usize) -> Result<Vec<u8>, AppError> {
    if !response.status().is_success() {
        return Err(if matches!(response.status().as_u16(), 401 | 403) {
            expired()
        } else {
            network()
        });
    }
    if response.content_length().is_some_and(|n| n > limit as u64) {
        return Err(protocol());
    }
    let mut output = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| network())?;
        if output.len() + chunk.len() > limit {
            return Err(protocol());
        }
        output.extend_from_slice(&chunk);
    }
    Ok(output)
}
fn account_json(bytes: &[u8]) -> Result<Value, AppError> {
    let data = bytes.strip_prefix(b"&&&START&&&").unwrap_or(bytes);
    serde_json::from_slice(data).map_err(|_| protocol())
}
fn checked_url(raw: &str, callback: bool) -> Result<Url, AppError> {
    let url = Url::parse(raw).map_err(|_| protocol())?;
    let valid_host = if callback {
        matches!(url.host_str(), Some("sts.api.io.mi.com" | "api.io.mi.com"))
    } else {
        // Xiaomi assigns regional account/long-poll hosts, e.g. sgp.lp.account.xiaomi.com.
        url.host_str().is_some_and(|host| {
            host == "account.xiaomi.com" || host.ends_with(".account.xiaomi.com")
        })
    };
    if url.scheme() != "https"
        || url.port_or_known_default() != Some(443)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || !valid_host
    {
        return Err(protocol());
    }
    Ok(url)
}

#[derive(Serialize, Deserialize)]
struct Auth {
    user_id: String,
    ssecurity: String,
    service_token: String,
    region: String,
}
impl Drop for Auth {
    fn drop(&mut self) {
        self.ssecurity.zeroize();
        self.service_token.zeroize();
        self.user_id.zeroize();
    }
}
impl Auth {
    fn validate(&self) -> Result<(), AppError> {
        if !region_valid(&self.region)
            || self.user_id.is_empty()
            || self.user_id.len() > 30
            || !self.user_id.bytes().all(|c| c.is_ascii_digit())
            || self.service_token.is_empty()
            || self.service_token.len() > 4096
            || self
                .service_token
                .bytes()
                .any(|c| c <= 32 || c >= 127 || c == b';')
            || !B64
                .decode(&self.ssecurity)
                .is_ok_and(|b| (16..=64).contains(&b.len()))
        {
            return Err(protocol());
        }
        Ok(())
    }
}
struct Pending {
    id: String,
    client: Client,
    poll: Url,
    deadline: Instant,
    region: String,
    remember: bool,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QrLogin {
    pub id: String,
    pub image: String,
    pub expires_in: u64,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginProgress {
    pub state: &'static str,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MiDevice {
    pub did: String,
    pub name: String,
    pub model: String,
    pub room: String,
    pub home: String,
    pub online: bool,
    pub camera: bool,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MiSnapshot {
    pub logged_in: bool,
    pub region: String,
    pub account_label: Option<String>,
    pub devices: Vec<MiDevice>,
    pub warnings: Vec<String>,
    pub updated_at: Option<u64>,
}
#[derive(Clone, Serialize)]
pub struct Choice {
    pub value: Value,
    pub description: String,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MiProperty {
    pub siid: u32,
    pub piid: u32,
    pub name: String,
    pub format: String,
    pub readable: bool,
    pub writable: bool,
    pub range: Option<Vec<f64>>,
    pub choices: Vec<Choice>,
    pub value: Option<Value>,
    pub unavailable: bool,
}
#[derive(Default)]
struct State {
    pending: Option<Pending>,
    auth: Option<Arc<Auth>>,
    devices: Vec<MiDevice>,
    warnings: Vec<String>,
    updated: Option<u64>,
    properties: HashMap<String, Vec<MiProperty>>,
    models: HashMap<String, String>,
}
#[derive(Clone)]
pub struct MiCloud {
    credentials: Arc<dyn CredentialStore>,
    state: Arc<Mutex<State>>,
    operation: Arc<Mutex<()>>,
    pub cameras: super::camera_stream::CameraStreams,
    #[cfg(test)]
    endpoint: Option<Url>,
}
impl MiCloud {
    pub fn new(credentials: Arc<dyn CredentialStore>) -> Self {
        Self {
            credentials,
            state: Arc::new(Mutex::new(State::default())),
            operation: Arc::new(Mutex::new(())),
            cameras: super::camera_stream::CameraStreams::default(),
            #[cfg(test)]
            endpoint: None,
        }
    }
    fn target(&self, raw: &str) -> Result<Url, AppError> {
        #[allow(unused_mut)]
        let mut url = Url::parse(raw).map_err(|_| protocol())?;
        #[cfg(test)]
        if let Some(endpoint) = &self.endpoint {
            url.set_scheme(endpoint.scheme()).map_err(|_| protocol())?;
            url.set_host(endpoint.host_str()).map_err(|_| protocol())?;
            url.set_port(endpoint.port()).map_err(|_| protocol())?;
        }
        Ok(url)
    }
    pub async fn snapshot(&self) -> MiSnapshot {
        let state = self.state.lock().await;
        MiSnapshot {
            logged_in: state.auth.is_some(),
            region: state
                .auth
                .as_ref()
                .map(|a| a.region.clone())
                .unwrap_or_else(|| "cn".into()),
            account_label: state.auth.as_ref().map(|a| {
                format!(
                    "小米账号 · 尾号 {}",
                    &a.user_id[a.user_id.len().saturating_sub(4)..]
                )
            }),
            devices: state.devices.clone(),
            warnings: state.warnings.clone(),
            updated_at: state.updated,
        }
    }
    pub async fn restore(&self) -> Result<MiSnapshot, AppError> {
        let _operation = self.operation.lock().await;
        if self.state.lock().await.auth.is_none() {
            if let Some(value) = self.credentials.read_optional(SLOT)? {
                let auth: Auth = serde_json::from_str(&value).map_err(|_| expired())?;
                auth.validate()?;
                self.state.lock().await.auth = Some(Arc::new(auth));
            }
        }
        Ok(self.snapshot().await)
    }
    pub async fn start_login(&self, region: String, remember: bool) -> Result<QrLogin, AppError> {
        if !region_valid(&region) {
            return Err(AppError::invalid_input("请选择米家 App 中使用的服务地区"));
        }
        let _operation = self.operation.lock().await;
        self.cameras.stop_all().await;
        self.state.lock().await.pending = None;
        let http = client()?;
        let device_id = uuid::Uuid::new_v4().simple().to_string();
        let response = http
            .get(self.target("https://account.xiaomi.com/longPolling/loginUrl")?)
            .header(
                reqwest::header::COOKIE,
                format!("sdkVersion=accountsdk-18.8.15; deviceId={device_id}"),
            )
            .query(&[
                ("_qrsize", "320".to_string()),
                ("qs", "%3Fsid%3Dxiaomiio%26_json%3Dtrue".into()),
                ("callback", "https://sts.api.io.mi.com/sts".into()),
                ("_hasLogo", "false".into()),
                ("sid", "xiaomiio".into()),
                ("serviceParam", "".into()),
                ("_locale", "zh_CN".into()),
                ("_dc", (now() * 1000).to_string()),
            ])
            .send()
            .await
            .map_err(|_| network())?;
        let data = account_json(&body(response, 128 * 1024).await?)?;
        let qr = checked_url(data["qr"].as_str().ok_or_else(protocol)?, false)?;
        let poll = checked_url(data["lp"].as_str().ok_or_else(protocol)?, false)?;
        let lifetime = data["timeout"].as_u64().unwrap_or(180);
        let lifetime = (if lifetime > 1000 {
            lifetime / 1000
        } else {
            lifetime
        })
        .clamp(10, 300);
        let image = body(
            http.get(self.target(qr.as_str())?)
                .send()
                .await
                .map_err(|_| network())?,
            512 * 1024,
        )
        .await?;
        if !image.starts_with(b"\x89PNG\r\n\x1a\n") {
            return Err(protocol());
        }
        let id = uuid::Uuid::new_v4().to_string();
        self.state.lock().await.pending = Some(Pending {
            id: id.clone(),
            client: http,
            poll,
            deadline: Instant::now() + Duration::from_secs(lifetime),
            region,
            remember,
        });
        Ok(QrLogin {
            id,
            image: format!("data:image/png;base64,{}", B64.encode(image)),
            expires_in: lifetime,
        })
    }
    pub async fn cancel_login(&self, id: &str) -> bool {
        let mut state = self.state.lock().await;
        if state.pending.as_ref().is_some_and(|p| p.id == id) {
            state.pending = None;
        }
        true
    }
    pub async fn poll_login(&self, id: &str) -> Result<LoginProgress, AppError> {
        let _operation = self.operation.lock().await;
        let (http, poll, region, remember) = {
            let mut state = self.state.lock().await;
            let pending = state
                .pending
                .as_ref()
                .filter(|p| p.id == id)
                .ok_or_else(|| error("MIJIA_QR_EXPIRED", "二维码已失效，请刷新二维码", false))?;
            if Instant::now() >= pending.deadline {
                state.pending = None;
                return Ok(LoginProgress { state: "expired" });
            }
            (
                pending.client.clone(),
                pending.poll.clone(),
                pending.region.clone(),
                pending.remember,
            )
        };
        let response = match http
            .get(self.target(poll.as_str())?)
            .timeout(Duration::from_secs(12))
            .send()
            .await
        {
            Ok(response) => response,
            Err(e) if e.is_timeout() => return Ok(LoginProgress { state: "waiting" }),
            Err(_) => return Err(network()),
        };
        if matches!(response.status().as_u16(), 408 | 504) {
            return Ok(LoginProgress { state: "waiting" });
        }
        let data = account_json(&body(response, 128 * 1024).await?)?;
        if data["code"].as_i64().is_some_and(|c| c != 0) {
            self.cancel_login(id).await;
            return Err(error(
                "MIJIA_QR_REJECTED",
                "扫码未完成或需要额外验证，请在手机确认后重新扫码",
                false,
            ));
        }
        let location = checked_url(data["location"].as_str().ok_or_else(protocol)?, true)?;
        let mut auth = Auth {
            user_id: data["userId"]
                .as_str()
                .map(str::to_string)
                .or_else(|| data["userId"].as_u64().map(|n| n.to_string()))
                .ok_or_else(protocol)?,
            ssecurity: data["ssecurity"].as_str().ok_or_else(protocol)?.into(),
            service_token: String::new(),
            region,
        };
        let mut target = location;
        // Every redirect is checked before sending. The token never reaches an arbitrary host.
        for _ in 0..5 {
            let response = http
                .get(self.target(target.as_str())?)
                .send()
                .await
                .map_err(|_| network())?;
            for cookie in response.cookies() {
                if cookie.name() == "serviceToken" {
                    auth.service_token = cookie.value().into();
                }
            }
            if response.status().is_redirection() {
                target = checked_url(
                    response
                        .headers()
                        .get(reqwest::header::LOCATION)
                        .and_then(|h| h.to_str().ok())
                        .ok_or_else(protocol)?,
                    true,
                )?;
            } else {
                let _ = body(response, 128 * 1024).await?;
                break;
            }
        }
        auth.validate()?;
        let mut state = self.state.lock().await;
        if !state
            .pending
            .as_ref()
            .is_some_and(|p| p.id == id && Instant::now() < p.deadline)
        {
            return Err(error(
                "MIJIA_QR_EXPIRED",
                "登录已取消或二维码已过期，请重新扫码",
                false,
            ));
        }
        if remember {
            let serialized = Zeroizing::new(serde_json::to_string(&auth).map_err(|_| protocol())?);
            self.credentials.write(SLOT, &serialized)?;
        } else {
            // A temporary login must not silently restore the previous remembered account.
            self.credentials.delete(SLOT)?;
        }
        *state = State {
            auth: Some(Arc::new(auth)),
            ..State::default()
        };
        Ok(LoginProgress {
            state: "authorized",
        })
    }
    pub async fn logout(&self) -> Result<bool, AppError> {
        // Invalidate a pending long poll immediately, even while it is awaiting HTTP.
        self.state.lock().await.pending = None;
        let _operation = self.operation.lock().await;
        self.cameras.stop_all().await;
        self.credentials.delete(SLOT)?;
        *self.state.lock().await = State::default();
        Ok(true)
    }
    async fn auth(&self) -> Result<Arc<Auth>, AppError> {
        self.state.lock().await.auth.clone().ok_or_else(expired)
    }
    async fn request(&self, auth: &Auth, path: &str, data: Value) -> Result<Value, AppError> {
        let mut nonce = [0u8; 12];
        rand::rng().fill_bytes(&mut nonce[..8]);
        nonce[8..].copy_from_slice(&((now() / 60) as u32).to_be_bytes());
        let (fields, key) = encrypted_fields(path, &auth.ssecurity, &nonce, &data)?;
        let host = if auth.region == "cn" {
            "api.io.mi.com".into()
        } else {
            format!("{}.api.io.mi.com", auth.region)
        };
        let response = client()?.post(self.target(&format!("https://{host}/app{path}"))?)
            .header("MIOT-ENCRYPT-ALGORITHM", "ENCRYPT-RC4")
            .header("X-XIAOMI-PROTOCAL-FLAG-CLI", "PROTOCAL-HTTP2")
            .header("Accept-Encoding", "identity")
            .header(reqwest::header::COOKIE, format!("userId={}; serviceToken={}; yetAnotherServiceToken={}; locale=zh_CN; timezone=GMT%2B08%3A00; channel=MI_APP_STORE", auth.user_id, auth.service_token, auth.service_token))
            .form(&fields).send().await.map_err(|_| network())?;
        let raw = Zeroizing::new(body(response, 8 * 1024 * 1024).await?);
        let decoded = if raw.starts_with(b"{") {
            raw
        } else {
            let mut bytes = B64.decode(raw.trim_ascii()).map_err(|_| protocol())?;
            crypt(&key, &mut bytes)?;
            Zeroizing::new(bytes)
        };
        let data: Value = serde_json::from_slice(&decoded).map_err(|_| protocol())?;
        match data["code"].as_i64() {
            Some(0) => Ok(data["result"].clone()),
            Some(-3 | -6) => Err(expired()),
            _ => Err(error(
                "MIJIA_API_REJECTED",
                "小米服务未接受请求，请检查账号地区、设备在线状态或稍后重试",
                false,
            )),
        }
    }
    pub async fn sync(&self) -> Result<MiSnapshot, AppError> {
        let _operation = self.operation.lock().await;
        let auth = self.auth().await?;
        let result = self.request(&auth, "/home/device_list", json!({"getVirtualModel":true,"getHuamiDevices":1,"get_split_device":false,"support_smart_home":true})).await;
        let list = match result {
            Ok(data) => data,
            Err(e) => {
                if e.code == "MIJIA_AUTH_EXPIRED" {
                    self.state.lock().await.auth = None;
                }
                return Err(e);
            }
        };
        let list = list["list"].as_array().ok_or_else(protocol)?;
        let homes = self.request(&auth, "/v2/homeroom/gethome_merged", json!({"fg":true,"fetch_share":true,"fetch_share_dev":true,"limit":300,"app_ver":7,"plat_form":0})).await;
        let mut rooms = HashMap::new();
        let mut warnings = Vec::new();
        if let Ok(homes) = homes {
            for home in homes["homelist"].as_array().into_iter().flatten() {
                for room in home["roomlist"].as_array().into_iter().flatten() {
                    for did in room["dids"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(Value::as_str)
                    {
                        rooms.insert(
                            did.to_string(),
                            (
                                home["name"].as_str().unwrap_or("").to_string(),
                                room["name"].as_str().unwrap_or("未分配房间").to_string(),
                            ),
                        );
                    }
                }
            }
        } else {
            warnings.push("房间信息暂时无法同步，设备列表仍可使用".into());
        }
        let mut devices = Vec::new();
        for item in list.iter().take(5000) {
            let Some(did) = item["did"]
                .as_str()
                .filter(|s| !s.is_empty() && s.len() <= 128)
            else {
                continue;
            };
            let model = item["model"].as_str().unwrap_or("");
            let (home, room) = rooms
                .get(did)
                .cloned()
                .unwrap_or_else(|| (String::new(), "未分配房间".into()));
            // Deliberately do not return device tokens, local IPs or raw cloud attributes to IPC.
            devices.push(MiDevice {
                did: did.into(),
                name: item["name"]
                    .as_str()
                    .unwrap_or("未命名设备")
                    .chars()
                    .take(100)
                    .collect(),
                model: model.chars().take(128).collect(),
                room,
                home,
                online: item["isOnline"].as_bool().unwrap_or(false),
                camera: model.contains(".camera.") || model.contains(".ipcamera."),
            });
        }
        if devices.len() >= 5000 {
            warnings.push("设备数量超过单次同步上限，列表可能不完整".into());
        }
        let mut state = self.state.lock().await;
        state
            .properties
            .retain(|did, _| devices.iter().any(|d| &d.did == did));
        state.devices = devices;
        state.warnings = warnings;
        state.updated = Some(now());
        drop(state);
        Ok(self.snapshot().await)
    }
    async fn spec(&self, did: &str) -> Result<Vec<MiProperty>, AppError> {
        let (model, cached) = {
            let state = self.state.lock().await;
            let device =
                state.devices.iter().find(|d| d.did == did).ok_or_else(|| {
                    AppError::invalid_input("设备不在当前米家账号中，请先同步设备")
                })?;
            (device.model.clone(), state.properties.get(did).cloned())
        };
        if let Some(properties) = cached {
            return Ok(properties);
        }
        if self.state.lock().await.models.is_empty() {
            let raw = body(
                client()?
                    .get(self.target("https://miot-spec.org/miot-spec-v2/instances?status=all")?)
                    .send()
                    .await
                    .map_err(|_| network())?,
                32 * 1024 * 1024,
            )
            .await?;
            let data: Value = serde_json::from_slice(&raw).map_err(|_| protocol())?;
            let mut state = self.state.lock().await;
            for item in data["instances"].as_array().ok_or_else(protocol)? {
                if let (Some(model), Some(urn)) = (item["model"].as_str(), item["type"].as_str()) {
                    state.models.insert(model.into(), urn.into());
                }
            }
        }
        let urn = self
            .state
            .lock()
            .await
            .models
            .get(&model)
            .cloned()
            .ok_or_else(|| {
                error(
                    "MIJIA_UNSUPPORTED",
                    "已发现此设备，但尚无可用的 MIoT 控制说明；它可能只支持旧协议或蓝牙网关",
                    false,
                )
            })?;
        let raw = body(
            client()?
                .get(self.target("https://miot-spec.org/miot-spec-v2/instance")?)
                .query(&[("type", urn)])
                .send()
                .await
                .map_err(|_| network())?,
            2 * 1024 * 1024,
        )
        .await?;
        let data: Value = serde_json::from_slice(&raw).map_err(|_| protocol())?;
        let properties = parse_spec(&data);
        self.state
            .lock()
            .await
            .properties
            .insert(did.into(), properties.clone());
        Ok(properties)
    }
    async fn read_properties(
        &self,
        auth: &Auth,
        did: &str,
        mut properties: Vec<MiProperty>,
    ) -> Result<Vec<MiProperty>, AppError> {
        let params: Vec<Value> = properties
            .iter()
            .filter(|p| p.readable)
            .map(|p| json!({"did":did,"siid":p.siid,"piid":p.piid}))
            .collect();
        for chunk in params.chunks(30) {
            let values = self
                .request(auth, "/miotspec/prop/get", json!({"params":chunk}))
                .await?;
            for p in properties.iter_mut().filter(|p| p.readable) {
                if let Some(value) = values
                    .as_array()
                    .ok_or_else(protocol)?
                    .iter()
                    .find(|v| v["siid"] == p.siid && v["piid"] == p.piid)
                {
                    p.unavailable = value["code"].as_i64() != Some(0);
                    p.value = if p.unavailable {
                        None
                    } else {
                        Some(value["value"].clone())
                    };
                }
            }
        }
        Ok(properties)
    }
    pub async fn properties(&self, did: &str) -> Result<Vec<MiProperty>, AppError> {
        let _operation = self.operation.lock().await;
        let auth = self.auth().await?;
        let properties = self.spec(did).await?;
        self.read_properties(&auth, did, properties).await
    }
    pub async fn camera_probe(
        &self,
        did: &str,
    ) -> Result<super::mijia_camera::CameraProbeReport, AppError> {
        use super::mijia_camera;
        let _operation = self.operation.lock().await;
        let auth = self.auth().await?;
        {
            let state = self.state.lock().await;
            let device = state.devices.iter().find(|d| d.did == did).ok_or_else(|| {
                AppError::invalid_input("摄像头不在当前账号设备列表中，请先同步设备")
            })?;
            if !device.camera || device.model != mijia_camera::TARGET_MODEL {
                return Err(AppError::invalid_input(
                    "当前检测先适配 chuangmi.camera.039a01",
                ));
            }
            if !device.online {
                return Err(error(
                    "MIJIA_DEVICE_OFFLINE",
                    "摄像头已离线，请先检查设备状态",
                    true,
                ));
            }
        }
        let data = self.request(&auth, "/v2/device/miss_get_vendor", json!({
            "app_pubkey":mijia_camera::challenge_public_key(), "did":did, "support_vendors":"TUTK_CS2_MTP"
        })).await?;
        mijia_camera::report(&data, now())
    }
    pub async fn camera_remote_start(
        &self,
        did: &str,
    ) -> Result<super::camera_stream::StreamStatus, AppError> {
        use super::mijia_camera;
        let _operation = self.operation.lock().await;
        let auth = self.auth().await?;
        {
            let state = self.state.lock().await;
            let device = state.devices.iter().find(|d| d.did == did).ok_or_else(|| {
                AppError::invalid_input("摄像头不在当前账号设备列表中，请先同步设备")
            })?;
            if !device.camera || device.model != mijia_camera::TARGET_MODEL {
                return Err(AppError::invalid_input(
                    "当前远程测试仅适配 chuangmi.camera.039a01",
                ));
            }
            if !device.online {
                return Err(error(
                    "MIJIA_DEVICE_OFFLINE",
                    "摄像头已离线，请先检查设备状态",
                    true,
                ));
            }
        }
        let cloud = self.clone();
        let device = did.to_owned();
        self.cameras.launch(did,async move {
            let challenge=mijia_camera::Challenge::default();
            let mut data=cloud.request(&auth,"/v2/device/miss_get_vendor",json!({"app_pubkey":challenge.public,"did":device,"support_vendors":"TUTK_CS2_MTP"})).await?;
            let result=mijia_camera::remote_grant(&data,challenge);
            fn clear(v:&mut Value) {match v {Value::String(s)=>s.zeroize(),Value::Array(a)=>a.iter_mut().for_each(clear),Value::Object(o)=>o.values_mut().for_each(clear),_=>{}}}
            clear(&mut data);
            result
        }).await
    }
    pub async fn set_property(
        &self,
        did: &str,
        siid: u32,
        piid: u32,
        value: Value,
    ) -> Result<Vec<MiProperty>, AppError> {
        let _operation = self.operation.lock().await;
        let auth = self.auth().await?;
        let properties = self.spec(did).await?;
        let device_online = self
            .state
            .lock()
            .await
            .devices
            .iter()
            .any(|d| d.did == did && d.online);
        if !device_online {
            return Err(error(
                "MIJIA_DEVICE_OFFLINE",
                "设备已离线，请检查电源或网关后刷新",
                true,
            ));
        }
        let property = properties
            .iter()
            .find(|p| p.siid == siid && p.piid == piid)
            .ok_or_else(|| AppError::invalid_input("设备未声明此控制能力"))?;
        validate_value(property, &value)?;
        let result = self
            .request(
                &auth,
                "/miotspec/prop/set",
                json!({"params":[{"did":did,"siid":siid,"piid":piid,"value":value}]}),
            )
            .await?;
        let accepted = result
            .as_array()
            .and_then(|a| {
                a.iter()
                    .find(|v| v["siid"] == siid && v["piid"] == piid && v["did"] == did)
            })
            .is_some_and(|v| v["code"].as_i64() == Some(0));
        if !accepted {
            return Err(error(
                "MIJIA_CONTROL_REJECTED",
                "设备未接受操作，请刷新状态后重试",
                false,
            ));
        }
        // Read the device's reported values; never turn a successful request into an invented state.
        self.read_properties(&auth, did, properties)
            .await
            .map_err(|_| {
                error(
                    "MIJIA_STATE_PENDING",
                    "指令已接受，状态尚未确认，请刷新设备状态",
                    true,
                )
            })
    }
}

fn crypt(key: &[u8], bytes: &mut [u8]) -> Result<(), AppError> {
    let mut cipher = Rc4::<U32>::new_from_slice(key).map_err(|_| protocol())?;
    cipher.apply_keystream(&mut [0u8; 1024]);
    cipher.apply_keystream(bytes);
    Ok(())
}
fn sign(path: &str, fields: &BTreeMap<String, String>, key: &[u8]) -> String {
    let mut parts = vec!["POST".to_string(), path.to_string()];
    parts.extend(fields.iter().map(|(k, v)| format!("{k}={v}")));
    parts.push(B64.encode(key));
    B64.encode(Sha1::digest(parts.join("&").as_bytes()))
}
fn encrypted_fields(
    path: &str,
    security: &str,
    nonce: &[u8],
    data: &Value,
) -> Result<(BTreeMap<String, String>, Zeroizing<Vec<u8>>), AppError> {
    let mut raw = Zeroizing::new(B64.decode(security).map_err(|_| protocol())?);
    raw.extend_from_slice(nonce);
    let key = Zeroizing::new(Sha256::digest(&*raw).to_vec());
    let mut fields = BTreeMap::from([("data".into(), data.to_string())]);
    fields.insert("rc4_hash__".into(), sign(path, &fields, &key));
    for value in fields.values_mut() {
        let mut bytes = Zeroizing::new(value.as_bytes().to_vec());
        crypt(&key, &mut bytes)?;
        *value = B64.encode(&*bytes);
    }
    fields.insert("signature".into(), sign(path, &fields, &key));
    fields.insert("ssecurity".into(), security.into());
    fields.insert("_nonce".into(), B64.encode(nonce));
    Ok((fields, key))
}
fn parse_spec(data: &Value) -> Vec<MiProperty> {
    const SAFE: &[&str] = &[
        "on",
        "brightness",
        "color-temperature",
        "target-temperature",
        "temperature",
        "relative-humidity",
        "mode",
        "fan-level",
        "horizontal-swing",
        "vertical-swing",
        "battery-level",
        "status",
        "pm2.5-density",
        "air-quality",
        "speed-level",
        "fault",
    ];
    let mut output = Vec::new();
    for service in data["services"].as_array().into_iter().flatten() {
        for property in service["properties"].as_array().into_iter().flatten() {
            let kind = property["type"]
                .as_str()
                .unwrap_or("")
                .split(':')
                .nth(3)
                .unwrap_or("");
            if !SAFE.contains(&kind) {
                continue;
            }
            let (Some(siid), Some(piid)) = (service["iid"].as_u64(), property["iid"].as_u64())
            else {
                continue;
            };
            if siid > u32::MAX as u64 || piid > u32::MAX as u64 {
                continue;
            }
            let access: Vec<&str> = property["access"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .collect();
            let format = property["format"].as_str().unwrap_or("");
            let choices: Vec<Choice> = property["value-list"]
                .as_array()
                .into_iter()
                .flatten()
                .take(100)
                .filter_map(|v| {
                    Some(Choice {
                        value: v.get("value")?.clone(),
                        description: v["description"].as_str()?.chars().take(100).collect(),
                    })
                })
                .collect();
            let range = property["value-range"]
                .as_array()
                .filter(|r| r.len() == 3)
                .and_then(|r| r.iter().map(Value::as_f64).collect::<Option<Vec<f64>>>())
                .filter(|r| r[0] <= r[1] && r[2] > 0.0);
            let writable = access.contains(&"write")
                && (format == "bool"
                    || (!choices.is_empty() || range.is_some())
                        && matches!(
                            format,
                            "uint8" | "uint16" | "uint32" | "int8" | "int16" | "int32" | "float"
                        ));
            output.push(MiProperty {
                siid: siid as u32,
                piid: piid as u32,
                name: format!(
                    "{} · {}",
                    service["description"].as_str().unwrap_or("设备"),
                    property["description"].as_str().unwrap_or(kind)
                ),
                format: format.into(),
                readable: access.contains(&"read"),
                writable,
                range,
                choices,
                value: None,
                unavailable: true,
            });
            if output.len() >= 100 {
                return output;
            }
        }
    }
    output
}
fn validate_value(property: &MiProperty, value: &Value) -> Result<(), AppError> {
    let invalid = || AppError::invalid_input("操作值不符合设备声明的类型、范围或步长");
    if !property.writable {
        return Err(invalid());
    }
    if property.format == "bool" {
        return if value.is_boolean() {
            Ok(())
        } else {
            Err(invalid())
        };
    }
    let number = value
        .as_f64()
        .filter(|v| v.is_finite())
        .ok_or_else(invalid)?;
    if property.format != "float" && number.fract() != 0.0 {
        return Err(invalid());
    }
    if !property.choices.is_empty() && !property.choices.iter().any(|c| c.value == *value) {
        return Err(invalid());
    }
    if let Some(range) = &property.range {
        let steps = (number - range[0]) / range[2];
        if number < range[0] || number > range[1] || (steps - steps.round()).abs() > 1e-5 {
            return Err(invalid());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::security::MemoryCredentials;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };

    const SECURITY: &str = "AAECAwQFBgcICQoLDA0ODw==";
    fn spec_fixture() -> Value {
        json!({"services":[{"iid":2,"description":"Light","properties":[
            {"iid":1,"type":"urn:miot-spec-v2:property:on:00000006:1","description":"Power","format":"bool","access":["read","write"]},
            {"iid":2,"type":"urn:miot-spec-v2:property:brightness:0000000D:1","description":"Brightness","format":"uint8","access":["read","write"],"value-range":[1,99,2]},
            {"iid":3,"type":"urn:miot-spec-v2:property:mode:1:1","description":"Mode","format":"uint8","access":["read","write"],"value-list":[{"value":1,"description":"Auto"},{"value":2,"description":"Manual"}]},
            {"iid":4,"type":"urn:miot-spec-v2:property:password:1:1","description":"Password","format":"string","access":["read","write"]}
        ]}]})
    }
    #[test]
    fn crypto_matches_independent_pycryptodome_rc4_drop1024_vector() {
        let (fields, key) = encrypted_fields(
            "/miotspec/prop/get",
            SECURITY,
            &(0..12).collect::<Vec<u8>>(),
            &json!({"params":[{"did":"123","piid":1,"siid":2}]}),
        )
        .unwrap();
        assert_eq!(
            B64.encode(&*key),
            "Nz6YMO70n75ITJeXTWpiyXXLp6IBZ0yT0/hEJB0FDOg="
        );
        assert_eq!(
            fields["data"],
            "Gjcp2tvQQivbG8/E8HqKAtVic7Vl3njHMJleFDCiK1k+PJ/i5amdjDEnfMw="
        );
        assert_eq!(
            fields["rc4_hash__"],
            "CVsviMHIfBOURdPnvizWHs5rCOxutSPfWqIPQA=="
        );
        assert_eq!(fields["signature"], "rlGs0yRdfMVtDzRV9+OGL8bnaTY=");
    }
    #[test]
    fn authorization_urls_reject_non_xiaomi_destinations_and_credential_urls() {
        for url in [
            "http://account.xiaomi.com/qr",
            "https://account.xiaomi.com.evil.test/qr",
            "https://evilaccount.xiaomi.com/qr",
            "https://account.xiaomi.com.evil.test/lp/s",
            "https://user@account.xiaomi.com/qr",
            "https://account.xiaomi.com:444/qr",
            "https://127.0.0.1/qr",
            "https://account.xiaomi.com/qr#fragment",
        ] {
            assert!(checked_url(url, false).is_err(), "{url}");
        }
        assert!(
            checked_url(
                "https://account.xiaomi.com/longPolling/login?ticket=fixture",
                false
            )
            .is_ok()
        );
        assert!(checked_url("https://sts.api.io.mi.com/sts?ticket=fixture", true).is_ok());
        assert!(checked_url("https://account.xiaomi.com/sts", true).is_err());
        assert!(checked_url("https://sgp.lp.account.xiaomi.com/lp/s?k=fixture", false).is_ok());
        assert!(checked_url("https://sgp.account.xiaomi.com/longPolling/login", false).is_ok());
    }
    #[test]
    fn spec_filters_secrets_and_checks_types_steps_enums_and_write_access() {
        let mut properties = parse_spec(&spec_fixture());
        assert_eq!(properties.len(), 3);
        assert!(validate_value(&properties[0], &json!(true)).is_ok());
        assert!(validate_value(&properties[0], &json!(1)).is_err());
        assert!(validate_value(&properties[1], &json!(3)).is_ok());
        for value in [json!(2), json!(101), json!(-1), json!(1.5), json!("3")] {
            assert!(validate_value(&properties[1], &value).is_err());
        }
        assert!(validate_value(&properties[2], &json!(2)).is_ok());
        assert!(validate_value(&properties[2], &json!(3)).is_err());
        properties[0].writable = false;
        assert!(validate_value(&properties[0], &json!(true)).is_err());
    }

    async fn server(
        malicious_callback: bool,
        delay_poll: bool,
    ) -> (Url, tokio::task::JoinHandle<()>, Arc<AtomicUsize>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = Url::parse(&format!("http://{}", listener.local_addr().unwrap())).unwrap();
        let writes = Arc::new(AtomicUsize::new(0));
        let counts = writes.clone();
        let task = tokio::spawn(async move {
            loop {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut raw = Vec::new();
                let mut buffer = [0u8; 4096];
                let (head, bytes) = loop {
                    let n = stream.read(&mut buffer).await.unwrap();
                    if n == 0 {
                        return;
                    }
                    raw.extend_from_slice(&buffer[..n]);
                    if let Some(split) = raw.windows(4).position(|b| b == b"\r\n\r\n") {
                        let head = String::from_utf8_lossy(&raw[..split]).to_string();
                        let length: usize = head
                            .lines()
                            .filter_map(|l| l.split_once(':'))
                            .find(|(k, _)| k.eq_ignore_ascii_case("content-length"))
                            .map(|(_, v)| v.trim().parse().unwrap())
                            .unwrap_or(0);
                        if raw.len() >= split + 4 + length {
                            break (head, raw[split + 4..split + 4 + length].to_vec());
                        }
                    }
                    assert!(raw.len() < 128 * 1024);
                };
                let route = head
                    .lines()
                    .next()
                    .unwrap()
                    .split_whitespace()
                    .nth(1)
                    .unwrap()
                    .split('?')
                    .next()
                    .unwrap();
                let mut headers = String::new();
                let response = match route {
                    "/longPolling/loginUrl" => format!("&&&START&&&{}",json!({"qr":"https://account.xiaomi.com/qr","lp":"https://sgp.lp.account.xiaomi.com/lp/s?k=fixture","timeout":180})).into_bytes(),
                    "/qr" => b"\x89PNG\r\n\x1a\nfixture".to_vec(),
                    "/lp/s" => {
                        if delay_poll { tokio::time::sleep(Duration::from_millis(150)).await; }
                        json!({"code":0,"userId":"12345678","ssecurity":SECURITY,"location":if malicious_callback {"https://evil.test/steal"} else {"https://sts.api.io.mi.com/sts"}}).to_string().into_bytes()
                    },
                    "/sts" => { headers = "Set-Cookie: serviceToken=fixture-service-token; Path=/\r\n".into(); b"ok".to_vec() },
                    "/miot-spec-v2/instances" => json!({"instances":[{"model":"fixture.light.one","type":"urn:fixture:light"}]}).to_string().into_bytes(),
                    "/miot-spec-v2/instance" => spec_fixture().to_string().into_bytes(),
                    route if route.starts_with("/app/") => {
                        assert!(head.to_lowercase().contains("serviceToken=fixture-service-token".to_lowercase().as_str()));
                        let fields: BTreeMap<String,String> = url::form_urlencoded::parse(&bytes).into_owned().collect();
                        let mut material = B64.decode(SECURITY).unwrap(); material.extend(B64.decode(&fields["_nonce"]).unwrap());
                        let key = Sha256::digest(material);
                        let mut request = B64.decode(&fields["data"]).unwrap(); crypt(&key,&mut request).unwrap();
                        let request: Value = serde_json::from_slice(&request).unwrap();
                        let result = match route {
                            "/app/v2/device/miss_get_vendor" => {
                                counts.fetch_add(1, Ordering::SeqCst);
                                assert_eq!(request["did"], "123");
                                assert_eq!(request["support_vendors"], "TUTK_CS2_MTP");
                                assert_eq!(request["app_pubkey"].as_str().unwrap().len(),64);
                                json!({"vendor":{"vendor":4,"vendor_params":{"p2p_id":"private-id"}},"public_key":"ab".repeat(32),"sign":"private-sign"})
                            },
                            "/app/home/device_list" => json!({"list":[{"did":"123","name":"书房灯","model":"fixture.light.one","isOnline":true,"token":"must-not-leak","localip":"192.168.1.2"}]}),
                            "/app/v2/homeroom/gethome_merged" => json!({"homelist":[{"name":"我的家","roomlist":[{"name":"书房","dids":["123"]}]}]}),
                            "/app/miotspec/prop/get" => Value::Array(request["params"].as_array().unwrap().iter().map(|p| json!({"did":"123","siid":p["siid"],"piid":p["piid"],"code":0,"value":if p["piid"] == 1 {json!(false)} else {json!(1)}})).collect()),
                            "/app/miotspec/prop/set" => { counts.fetch_add(1,Ordering::SeqCst); assert_eq!(request["params"][0]["value"],true); json!([{"did":"123","siid":2,"piid":1,"code":0}]) },
                            _ => panic!("Unexpected API route: {route}"),
                        };
                        let mut result = json!({"code":0,"result":result}).to_string().into_bytes(); crypt(&key,&mut result).unwrap(); B64.encode(result).into_bytes()
                    },
                    _ => panic!("Unexpected route: {route}"),
                };
                let prefix = format!(
                    "HTTP/1.1 200 OK\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n",
                    response.len()
                );
                let _ = stream.write_all(prefix.as_bytes()).await;
                let _ = stream.write_all(&response).await;
            }
        });
        (endpoint, task, writes)
    }
    #[tokio::test]
    async fn qr_to_remembered_session_rooms_and_signed_control_without_optimistic_state() {
        let (endpoint, task, writes) = server(false, false).await;
        let credentials = Arc::new(MemoryCredentials::default());
        let mut cloud = MiCloud::new(credentials.clone());
        cloud.endpoint = Some(endpoint);
        assert!(!cloud.restore().await.unwrap().logged_in);
        let qr = cloud.start_login("cn".into(), true).await.unwrap();
        assert!(qr.image.starts_with("data:image/png;base64,"));
        assert_eq!(cloud.poll_login(&qr.id).await.unwrap().state, "authorized");
        let restored = MiCloud::new(credentials.clone()).restore().await.unwrap();
        assert!(restored.logged_in);
        let snapshot = cloud.sync().await.unwrap();
        assert_eq!(snapshot.devices[0].room, "书房");
        let public = serde_json::to_string(&snapshot).unwrap();
        for secret in [
            "must-not-leak",
            "192.168.1.2",
            "fixture-service-token",
            SECURITY,
        ] {
            assert!(!public.contains(secret));
        }
        let properties = cloud.properties("123").await.unwrap();
        assert_eq!(properties.len(), 3);
        assert!(
            cloud
                .set_property("another-account-device", 2, 1, json!(true))
                .await
                .is_err()
        );
        assert!(cloud.set_property("123", 2, 2, json!(2)).await.is_err());
        assert_eq!(writes.load(Ordering::SeqCst), 0);
        let updated = cloud.set_property("123", 2, 1, json!(true)).await.unwrap();
        assert_eq!(writes.load(Ordering::SeqCst), 1);
        assert_eq!(updated[0].value, Some(json!(false)));
        assert!(cloud.logout().await.unwrap());
        assert!(!cloud.restore().await.unwrap().logged_in);
        assert!(!MiCloud::new(credentials).restore().await.unwrap().logged_in);
        task.abort();
    }
    #[tokio::test]
    async fn canceled_in_flight_login_and_untrusted_callback_cannot_save_credentials() {
        for malicious in [false, true] {
            let (endpoint, task, _) = server(malicious, true).await;
            let credentials = Arc::new(MemoryCredentials::default());
            let mut cloud = MiCloud::new(credentials.clone());
            cloud.endpoint = Some(endpoint);
            let qr = cloud.start_login("cn".into(), true).await.unwrap();
            let polling = cloud.clone();
            let id = qr.id.clone();
            let poll = tokio::spawn(async move { polling.poll_login(&id).await });
            if !malicious {
                tokio::time::sleep(Duration::from_millis(30)).await;
                cloud.cancel_login(&qr.id).await;
            }
            assert!(poll.await.unwrap().is_err());
            assert!(credentials.read_optional(SLOT).unwrap().is_none());
            assert!(!cloud.snapshot().await.logged_in);
            task.abort();
        }
    }
    #[tokio::test]
    async fn expired_qr_never_polls_or_authenticates() {
        let (endpoint, task, _) = server(false, false).await;
        let mut cloud = MiCloud::new(Arc::new(MemoryCredentials::default()));
        cloud.endpoint = Some(endpoint);
        let qr = cloud.start_login("cn".into(), true).await.unwrap();
        cloud.state.lock().await.pending.as_mut().unwrap().deadline =
            Instant::now() - Duration::from_secs(1);
        assert_eq!(cloud.poll_login(&qr.id).await.unwrap().state, "expired");
        assert!(!cloud.snapshot().await.logged_in);
        task.abort();
    }

    #[tokio::test]
    async fn camera_probe_checks_account_model_and_online_status_before_requesting_a_grant() {
        let (endpoint, task, calls) = server(false, false).await;
        let mut cloud = MiCloud::new(Arc::new(MemoryCredentials::default()));
        cloud.endpoint = Some(endpoint);
        let qr = cloud.start_login("cn".into(), true).await.unwrap();
        cloud.poll_login(&qr.id).await.unwrap();
        assert!(cloud.camera_probe("another-account-device").await.is_err());
        cloud.state.lock().await.devices.push(MiDevice {
            did: "123".into(),
            name: "fixture".into(),
            model: "other.camera.model".into(),
            room: "".into(),
            home: "".into(),
            online: true,
            camera: true,
        });
        assert!(cloud.camera_probe("123").await.is_err());
        {
            let mut state = cloud.state.lock().await;
            state.devices[0].model = super::super::mijia_camera::TARGET_MODEL.into();
            state.devices[0].online = false;
        }
        assert!(cloud.camera_probe("123").await.is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        cloud.state.lock().await.devices[0].online = true;
        let probe = cloud.camera_probe("123").await.unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(probe.transport, "cs2");
        assert!(!probe.remote_playback_verified);
        assert!(!serde_json::to_string(&probe).unwrap().contains("private-"));
        task.abort();
    }

    #[tokio::test]
    async fn remote_start_requires_owned_online_model_and_logout_cancels_sessions() {
        let (endpoint, task, calls) = server(false, false).await;
        let mut cloud = MiCloud::new(Arc::new(MemoryCredentials::default()));
        cloud.endpoint = Some(endpoint);
        assert!(cloud.camera_remote_start("123").await.is_err());
        let qr = cloud.start_login("cn".into(), true).await.unwrap();
        cloud.poll_login(&qr.id).await.unwrap();
        assert!(cloud.camera_remote_start("other-device").await.is_err());
        cloud.state.lock().await.devices.push(MiDevice {
            did: "123".into(),
            name: "fixture".into(),
            model: super::super::mijia_camera::TARGET_MODEL.into(),
            home: "".into(),
            room: "".into(),
            online: false,
            camera: true,
        });
        assert!(cloud.camera_remote_start("123").await.is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        cloud.state.lock().await.devices[0].online = true;
        let session = cloud.camera_remote_start("123").await.unwrap();
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let st = cloud.cameras.status(&session.session_id).await.unwrap();
                if st.stage == "failed" {
                    assert!(!serde_json::to_string(&st).unwrap().contains("private-"));
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        cloud.logout().await.unwrap();
        assert!(cloud.cameras.status(&session.session_id).await.is_err());
        task.abort();
    }

    struct FailingCredentials;
    impl CredentialStore for FailingCredentials {
        fn read(&self, _: &str) -> Result<Zeroizing<String>, AppError> {
            Err(expired())
        }
        fn read_optional(&self, _: &str) -> Result<Option<Zeroizing<String>>, AppError> {
            Ok(None)
        }
        fn write(&self, _: &str, _: &str) -> Result<(), AppError> {
            Err(error("CREDENTIAL_STORE_FAILED", "凭据服务不可用", false))
        }
        fn delete(&self, _: &str) -> Result<(), AppError> {
            Ok(())
        }
    }
    #[tokio::test]
    async fn credential_failure_does_not_commit_login_but_temporary_login_remains_possible() {
        let (endpoint, task, _) = server(false, false).await;
        let mut cloud = MiCloud::new(Arc::new(FailingCredentials));
        cloud.endpoint = Some(endpoint);
        let qr = cloud.start_login("cn".into(), true).await.unwrap();
        let failure = cloud.poll_login(&qr.id).await.err().unwrap();
        assert_eq!(failure.code, "CREDENTIAL_STORE_FAILED");
        assert!(!cloud.snapshot().await.logged_in);
        let qr = cloud.start_login("cn".into(), false).await.unwrap();
        assert_eq!(cloud.poll_login(&qr.id).await.unwrap().state, "authorized");
        assert!(cloud.snapshot().await.logged_in);
        assert!(
            !MiCloud::new(Arc::new(FailingCredentials))
                .restore()
                .await
                .unwrap()
                .logged_in
        );
        task.abort();
    }
}
