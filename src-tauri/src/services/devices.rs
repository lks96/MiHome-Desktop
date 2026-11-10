use super::{
    catalog::DeviceCatalog,
    ha::normalize_url,
    session::{ConnectionState, EntitySummary, HaService},
};
use crate::{database::Database, errors::AppError};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::time::Duration;

#[derive(Clone, Debug, Serialize, Deserialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EntityPreference {
    pub entity_id: String,
    pub display_name: Option<String>,
    pub area_id_override: Option<String>,
    pub is_favorite: bool,
    pub is_hidden: bool,
    pub sort_order: i32,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ServiceRequest {
    pub entity_id: String,
    pub domain: String,
    pub service: String,
    pub service_data: Value,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceResult {
    pub accepted: bool,
    pub message: &'static str,
}

fn valid_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 255
        && value
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}
pub fn validate_entity_id(value: &str) -> Result<(), AppError> {
    if let Some((domain, entity)) = value.split_once('.') {
        if valid_identifier(domain) && valid_identifier(entity) {
            return Ok(());
        }
    }
    Err(AppError::invalid_input("实体 ID 格式错误"))
}
fn unsupported() -> AppError {
    AppError::new(
        "SERVICE_UNSUPPORTED",
        "当前实体未声明此操作或缺少对应能力",
        false,
    )
}
fn parameter_error() -> AppError {
    AppError::invalid_input("控制参数不符合设备能力或范围")
}
fn modes_contain(entity: &EntitySummary, key: &str, mode: &str) -> bool {
    entity.attributes[key]
        .as_array()
        .is_some_and(|items| items.iter().any(|item| item.as_str() == Some(mode)))
}
fn numeric(data: &Value, key: &str, min: f64, max: f64, step: Option<f64>) -> Result<(), AppError> {
    let value = data[key].as_f64().ok_or_else(parameter_error)?;
    if !value.is_finite()
        || !min.is_finite()
        || !max.is_finite()
        || min > max
        || value < min
        || value > max
    {
        return Err(parameter_error());
    }
    if let Some(step) = step {
        if !step.is_finite()
            || step <= 0.
            || (((value - min) / step).round() - (value - min) / step).abs() > 0.00001
        {
            return Err(parameter_error());
        }
    }
    Ok(())
}
/// Validate again in Rust. The frontend never chooses arbitrary service routes or targets.
pub fn validate_service(
    entity: &EntitySummary,
    catalog: &DeviceCatalog,
    request: &ServiceRequest,
) -> Result<(), AppError> {
    validate_entity_id(&request.entity_id)?;
    if request.entity_id != entity.entity_id
        || request.domain != entity.domain
        || !valid_identifier(&request.service)
    {
        return Err(parameter_error());
    }
    if entity.state == "unavailable" || (entity.state == "unknown" && entity.domain != "scene") {
        return Err(AppError::new(
            "ENTITY_UNAVAILABLE",
            "实体不可用，暂时无法控制",
            true,
        ));
    }
    if !catalog.services_loaded
        || !catalog
            .services
            .get(&request.domain)
            .is_some_and(|services| services.contains(&request.service))
    {
        return Err(unsupported());
    }
    let fields = request
        .service_data
        .as_object()
        .ok_or_else(parameter_error)?;
    let allowed: &[&str] = match (request.domain.as_str(), request.service.as_str()) {
        ("scene", "turn_on") => &[],
        ("light" | "switch" | "fan", "turn_off") | ("switch" | "fan", "turn_on") => &[],
        ("light", "turn_on") => &["brightness", "color_temp_kelvin", "rgb_color"],
        ("climate", "set_temperature") => &["temperature"],
        ("climate", "set_hvac_mode") => &["hvac_mode"],
        ("climate", "set_fan_mode") => &["fan_mode"],
        ("fan", "set_percentage") => &["percentage"],
        ("fan", "set_preset_mode") => &["preset_mode"],
        _ => return Err(unsupported()),
    };
    if fields.keys().any(|key| !allowed.contains(&key.as_str())) {
        return Err(parameter_error());
    }
    // All single-parameter setters require exactly one parameter.
    if request.service.starts_with("set_") && fields.len() != 1 {
        return Err(parameter_error());
    }
    let attr = &entity.attributes;
    let features = attr["supported_features"].as_u64().unwrap_or(0);
    for key in fields.keys() {
        match key.as_str() {
            "brightness" => {
                if attr.get("brightness").is_none()
                    || ![
                        "brightness",
                        "color_temp",
                        "hs",
                        "xy",
                        "rgb",
                        "rgbw",
                        "rgbww",
                        "white",
                    ]
                    .iter()
                    .any(|mode| modes_contain(entity, "supported_color_modes", mode))
                {
                    return Err(unsupported());
                }
                numeric(&request.service_data, key, 0., 255., Some(1.))?;
            }
            "color_temp_kelvin" => {
                if !modes_contain(entity, "supported_color_modes", "color_temp") {
                    return Err(unsupported());
                }
                let min = attr["min_color_temp_kelvin"]
                    .as_f64()
                    .ok_or_else(unsupported)?;
                let max = attr["max_color_temp_kelvin"]
                    .as_f64()
                    .ok_or_else(unsupported)?;
                numeric(&request.service_data, key, min, max, Some(1.))?;
                if fields.contains_key("rgb_color") {
                    return Err(parameter_error());
                }
            }
            "rgb_color" => {
                if !["hs", "xy", "rgb", "rgbw", "rgbww"]
                    .iter()
                    .any(|mode| modes_contain(entity, "supported_color_modes", mode))
                {
                    return Err(unsupported());
                }
                let rgb = request.service_data[key]
                    .as_array()
                    .ok_or_else(parameter_error)?;
                if rgb.len() != 3
                    || !rgb
                        .iter()
                        .all(|value| value.as_u64().is_some_and(|number| number <= 255))
                {
                    return Err(parameter_error());
                }
            }
            "temperature" => {
                if features & 1 == 0 || attr.get("temperature").is_none() {
                    return Err(unsupported());
                }
                numeric(
                    &request.service_data,
                    key,
                    attr["min_temp"].as_f64().ok_or_else(unsupported)?,
                    attr["max_temp"].as_f64().ok_or_else(unsupported)?,
                    attr["target_temp_step"].as_f64(),
                )?;
            }
            "hvac_mode" | "fan_mode" | "preset_mode" => {
                let list = match key.as_str() {
                    "hvac_mode" => "hvac_modes",
                    "fan_mode" => "fan_modes",
                    _ => "preset_modes",
                };
                if key == "fan_mode" && features & 8 == 0
                    || key == "preset_mode" && features & 8 == 0
                {
                    return Err(unsupported());
                }
                if !request.service_data[key]
                    .as_str()
                    .is_some_and(|value| modes_contain(entity, list, value))
                {
                    return Err(parameter_error());
                }
            }
            "percentage" => {
                if features & 1 == 0 || attr.get("percentage").is_none() {
                    return Err(unsupported());
                }
                numeric(
                    &request.service_data,
                    key,
                    0.,
                    100.,
                    attr["percentage_step"].as_f64().or(Some(1.)),
                )?;
            }
            _ => return Err(parameter_error()),
        }
    }
    Ok(())
}

impl Database {
    pub async fn entity_preferences(&self) -> Result<Vec<EntityPreference>, AppError> {
        sqlx::query_as("SELECT entity_id, display_name, area_id_override, is_favorite, is_hidden, sort_order FROM entity_overrides").fetch_all(&self.pool).await.map_err(|_| AppError::database("load_entity_preferences"))
    }
    pub async fn update_entity_preference(
        &self,
        mut preference: EntityPreference,
    ) -> Result<EntityPreference, AppError> {
        validate_entity_id(&preference.entity_id)?;
        preference.display_name = preference
            .display_name
            .map(|name| name.trim().to_owned())
            .filter(|name| !name.is_empty());
        if preference
            .display_name
            .as_ref()
            .is_some_and(|name| name.chars().count() > 80 || name.chars().any(char::is_control))
            || !(0..=10000).contains(&preference.sort_order)
        {
            return Err(AppError::invalid_input(
                "名称最多 80 个字符，排序需为 0 至 10000 的整数",
            ));
        }
        if preference
            .area_id_override
            .as_ref()
            .is_some_and(|id| id.is_empty() || id.len() > 128 || id.chars().any(char::is_control))
        {
            return Err(AppError::invalid_input("房间 ID 格式错误"));
        }
        sqlx::query("INSERT INTO entity_overrides(entity_id, display_name, area_id_override, is_favorite, is_hidden, sort_order) VALUES(?, ?, ?, ?, ?, ?) ON CONFLICT(entity_id) DO UPDATE SET display_name=excluded.display_name, area_id_override=excluded.area_id_override, is_favorite=excluded.is_favorite, is_hidden=excluded.is_hidden, sort_order=excluded.sort_order, updated_at=datetime('now')")
            .bind(&preference.entity_id).bind(&preference.display_name).bind(&preference.area_id_override).bind(preference.is_favorite).bind(preference.is_hidden).bind(preference.sort_order).execute(&self.pool).await.map_err(|_| AppError::database("save_entity_preference"))?;
        Ok(preference)
    }
}
impl HaService {
    pub async fn call_service(
        &self,
        database: &Database,
        request: ServiceRequest,
    ) -> Result<ServiceResult, AppError> {
        let _operation = self.operation.lock().await;
        if self.status().await.state != ConnectionState::Connected {
            return Err(AppError::new(
                "HA_WS_DISCONNECTED",
                "请先恢复实时连接再控制设备",
                true,
            ));
        }
        let entity = self
            .entities()
            .await
            .into_iter()
            .find(|entity| entity.entity_id == request.entity_id)
            .ok_or_else(|| AppError::new("ENTITY_NOT_FOUND", "实体不存在，请重新同步", false))?;
        validate_service(&entity, &self.catalog().await, &request)?;
        let connection = database
            .connection()
            .await?
            .ok_or_else(|| AppError::invalid_input("未配置连接"))?;
        if !self.bound_to(&connection.base_url).await {
            return Err(AppError::new(
                "HA_SYNC_FAILED",
                "连接配置已改变，请重新连接并同步后再控制",
                false,
            ));
        }
        let token = self.credentials.read(&connection.credential_ref)?;
        let settings = database.settings().await?;
        post_service(
            &connection.base_url,
            &token,
            settings.request_timeout,
            &request,
        )
        .await?;
        tracing::info!(
            domain = request.domain,
            service = request.service,
            "HA service call accepted"
        );
        // State changes come exclusively from the websocket, not from this HTTP response.
        Ok(ServiceResult {
            accepted: true,
            message: "HA 已处理服务请求；实际设备状态以实时同步为准",
        })
    }
}
async fn post_service(
    base_url: &str,
    token: &str,
    seconds: u32,
    request: &ServiceRequest,
) -> Result<(), AppError> {
    let url = normalize_url(base_url)?
        .join(&format!(
            "api/services/{}/{}",
            request.domain, request.service
        ))
        .map_err(|_| parameter_error())?;
    let mut data = request
        .service_data
        .as_object()
        .ok_or_else(parameter_error)?
        .clone();
    data.insert("entity_id".into(), json!(request.entity_id));
    let response = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(seconds as u64))
        .build()
        .map_err(|_| call_error())?
        .post(url)
        .bearer_auth(token)
        .json(&data)
        .send()
        .await
        .map_err(|_| call_error())?;
    if response.status() == 401 || response.status() == 403 {
        return Err(AppError::new(
            "HA_AUTH_INVALID",
            "设备控制认证失败，请更新访问令牌或检查权限",
            false,
        ));
    }
    if !response.status().is_success() {
        return Err(call_error());
    }
    let mut body = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| call_error())?;
        if body.len() + chunk.len() > 16 * 1024 * 1024 {
            return Err(call_error());
        }
        body.extend_from_slice(&chunk);
    }
    if !serde_json::from_slice::<Value>(&body)
        .map_err(|_| call_error())?
        .is_array()
    {
        return Err(call_error());
    }
    Ok(())
}
fn call_error() -> AppError {
    AppError::new(
        "SERVICE_CALL_FAILED",
        "服务调用失败或结果不明确，请先查看设备状态后再决定是否重试",
        false,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    fn entity(domain: &str, attributes: Value) -> EntitySummary {
        EntitySummary {
            entity_id: format!("{domain}.test"),
            name: "Test".into(),
            domain: domain.into(),
            state: "off".into(),
            unit: None,
            last_updated: "2026-11-10T01:00:00Z".into(),
            attributes,
        }
    }
    fn catalog(domain: &str, services: &[&str]) -> DeviceCatalog {
        DeviceCatalog {
            services_loaded: true,
            services: std::collections::BTreeMap::from([(
                domain.into(),
                services.iter().map(|v| (*v).into()).collect(),
            )]),
            ..DeviceCatalog::default()
        }
    }
    fn request(domain: &str, service: &str, data: Value) -> ServiceRequest {
        ServiceRequest {
            entity_id: format!("{domain}.test"),
            domain: domain.into(),
            service: service.into(),
            service_data: data,
        }
    }
    #[test]
    fn light_controls_enforce_capabilities_ranges_and_single_target() {
        let light = entity(
            "light",
            json!({"brightness":128,"supported_color_modes":["rgb","color_temp"],"min_color_temp_kelvin":2000,"max_color_temp_kelvin":6500}),
        );
        let registry = catalog("light", &["turn_on", "turn_off"]);
        for data in [
            json!({}),
            json!({"brightness":255}),
            json!({"color_temp_kelvin":3000}),
            json!({"rgb_color":[10,20,30]}),
        ] {
            validate_service(&light, &registry, &request("light", "turn_on", data)).unwrap();
        }
        for data in [
            json!({"brightness":256}),
            json!({"brightness":1.5}),
            json!({"brightness":-1}),
            json!({"color_temp_kelvin":1999}),
            json!({"rgb_color":[0,0,999]}),
            json!({"entity_id":"light.other"}),
            json!({"rgb_color":[1,2,3],"color_temp_kelvin":3000}),
        ] {
            assert!(
                validate_service(&light, &registry, &request("light", "turn_on", data)).is_err()
            );
        }
        let no_brightness = entity("light", json!({"supported_color_modes":["onoff"]}));
        assert_eq!(
            validate_service(
                &no_brightness,
                &registry,
                &request("light", "turn_on", json!({"brightness":10}))
            )
            .unwrap_err()
            .code,
            "SERVICE_UNSUPPORTED"
        );
        let mut offline = light.clone();
        offline.state = "unavailable".into();
        assert_eq!(
            validate_service(&offline, &registry, &request("light", "turn_on", json!({})))
                .unwrap_err()
                .code,
            "ENTITY_UNAVAILABLE"
        );
        assert!(
            validate_service(
                &light,
                &DeviceCatalog::default(),
                &request("light", "turn_on", json!({}))
            )
            .is_err()
        );
        let mut wrong = request("light", "turn_on", json!({}));
        wrong.domain = "switch".into();
        assert!(validate_service(&light, &registry, &wrong).is_err());
        wrong.domain = "light".into();
        wrong.entity_id = "light.test/../../api".into();
        assert!(validate_service(&light, &registry, &wrong).is_err());
    }
    #[test]
    fn climate_and_fan_validate_steps_enums_and_service_declarations() {
        let climate = entity(
            "climate",
            json!({"temperature":20,"min_temp":16,"max_temp":30,"target_temp_step":0.5,"hvac_modes":["off","cool"],"fan_modes":["auto","low"],"supported_features":9}),
        );
        let registry = catalog(
            "climate",
            &["set_temperature", "set_hvac_mode", "set_fan_mode"],
        );
        validate_service(
            &climate,
            &registry,
            &request("climate", "set_temperature", json!({"temperature":22.5})),
        )
        .unwrap();
        for data in [
            json!({"temperature":31}),
            json!({"temperature":22.3}),
            json!({}),
        ] {
            assert!(
                validate_service(
                    &climate,
                    &registry,
                    &request("climate", "set_temperature", data)
                )
                .is_err()
            );
        }
        validate_service(
            &climate,
            &registry,
            &request("climate", "set_hvac_mode", json!({"hvac_mode":"cool"})),
        )
        .unwrap();
        assert!(
            validate_service(
                &climate,
                &registry,
                &request("climate", "set_hvac_mode", json!({"hvac_mode":"heat"}))
            )
            .is_err()
        );
        let fan = entity(
            "fan",
            json!({"percentage":50,"percentage_step":25,"preset_modes":["sleep"],"supported_features":9}),
        );
        let registry = catalog("fan", &["set_percentage", "set_preset_mode"]);
        validate_service(
            &fan,
            &registry,
            &request("fan", "set_percentage", json!({"percentage":75})),
        )
        .unwrap();
        assert!(
            validate_service(
                &fan,
                &registry,
                &request("fan", "set_percentage", json!({"percentage":60}))
            )
            .is_err()
        );
        assert!(
            validate_service(
                &fan,
                &registry,
                &request("fan", "set_preset_mode", json!({"preset_mode":"turbo"}))
            )
            .is_err()
        );
    }
    #[tokio::test]
    async fn local_preferences_persist_and_invalid_changes_leave_saved_values() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("preferences.sqlite");
        let db = Database::open(&path).await.unwrap();
        let value = EntityPreference {
            entity_id: "switch.test".into(),
            display_name: Some("Desk".into()),
            area_id_override: Some("office".into()),
            is_favorite: true,
            is_hidden: true,
            sort_order: 10,
        };
        db.update_entity_preference(value.clone()).await.unwrap();
        let mut invalid = value.clone();
        invalid.sort_order = -1;
        assert!(db.update_entity_preference(invalid).await.is_err());
        db.close().await;
        let reopened = Database::open(&path).await.unwrap();
        let saved = reopened.entity_preferences().await.unwrap();
        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0].display_name.as_deref(), Some("Desk"));
        assert!(saved[0].is_favorite && saved[0].is_hidden);
        assert_eq!(saved[0].sort_order, 10);
        assert_eq!(saved[0].area_id_override.as_deref(), Some("office"));
        reopened.close().await;
    }
    #[tokio::test]
    async fn disconnected_control_is_rejected_before_any_request() {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::open(&dir.path().join("test.sqlite"))
            .await
            .unwrap();
        let service = HaService::new(
            Arc::new(crate::security::MemoryCredentials::default()),
            Arc::new(|_| {}),
        );
        let error = service
            .call_service(&db, request("switch", "turn_on", json!({})))
            .await
            .err()
            .unwrap();
        assert_eq!(error.code, "HA_WS_DISCONNECTED");
        db.close().await;
    }
    #[tokio::test]
    async fn http_control_targets_exact_entity_and_classifies_failures() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        for (status, body, code) in [
            ("200 OK", "[]", None),
            ("200 OK", "{}", Some("SERVICE_CALL_FAILED")),
            (
                "500 Error",
                "private error including credentials",
                Some("SERVICE_CALL_FAILED"),
            ),
            ("401 Unauthorized", "secret error", Some("HA_AUTH_INVALID")),
        ] {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let base_url = format!("http://{}/ha/", listener.local_addr().unwrap());
            let fixture = tokio::spawn(async move {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut bytes = Vec::new();
                loop {
                    let mut chunk = [0; 4096];
                    let count = socket.read(&mut chunk).await.unwrap();
                    assert!(count > 0);
                    bytes.extend_from_slice(&chunk[..count]);
                    if let Some(end) = bytes.windows(4).position(|v| v == b"\r\n\r\n") {
                        let headers = std::str::from_utf8(&bytes[..end])
                            .unwrap()
                            .to_ascii_lowercase();
                        let length: usize = headers
                            .lines()
                            .find_map(|line| line.strip_prefix("content-length: "))
                            .unwrap()
                            .parse()
                            .unwrap();
                        if bytes.len() >= end + 4 + length {
                            assert!(
                                headers.starts_with("post /ha/api/services/light/turn_on http/1.1")
                            );
                            assert!(headers.contains("authorization: bearer test-only-token"));
                            let payload: Value =
                                serde_json::from_slice(&bytes[end + 4..end + 4 + length]).unwrap();
                            assert_eq!(payload, json!({"entity_id":"light.test","brightness":128}));
                            break;
                        }
                    }
                }
                socket.write_all(format!("HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await.unwrap();
            });
            let result = post_service(
                &base_url,
                "test-only-token",
                2,
                &request("light", "turn_on", json!({"brightness":128})),
            )
            .await;
            match code {
                None => assert!(result.is_ok()),
                Some(code) => {
                    let error = result.err().unwrap();
                    assert_eq!(error.code, code);
                    assert!(!error.message.contains("secret"));
                }
            }
            fixture.await.unwrap();
        }
    }
}
