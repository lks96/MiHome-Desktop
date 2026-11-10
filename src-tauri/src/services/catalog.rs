use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceCatalog {
    pub areas: Vec<Area>,
    pub services: BTreeMap<String, Vec<String>>,
    pub entity_areas: BTreeMap<String, Option<String>>,
    pub warnings: Vec<String>,
    pub services_loaded: bool,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Area {
    #[serde(alias = "area_id")]
    pub area_id: String,
    pub name: String,
}
#[derive(Default)]
pub struct CatalogBuilder {
    pub catalog: DeviceCatalog,
    entities: Vec<Value>,
    devices: Vec<Value>,
}
impl CatalogBuilder {
    pub fn apply(&mut self, message: &Value) -> Option<DeviceCatalog> {
        let id = message["id"].as_u64()?;
        let key = match id {
            10 => "services",
            11 => "areas",
            12 => "entities",
            13 => "devices",
            _ => return None,
        };
        let result = &message["result"];
        let valid = if message["success"] != true {
            false
        } else {
            match id {
                10 => {
                    if let Some(domains) = result.as_object() {
                        self.catalog.services = domains
                            .iter()
                            .filter_map(|(domain, services)| {
                                services.as_object().map(|services| {
                                    (domain.clone(), services.keys().cloned().collect())
                                })
                            })
                            .collect();
                        self.catalog.services_loaded = true;
                        true
                    } else {
                        false
                    }
                }
                11 => match serde_json::from_value::<Vec<Area>>(result.clone()) {
                    Ok(areas) => {
                        self.catalog.areas = areas;
                        true
                    }
                    Err(_) => false,
                },
                12 => match result.as_array() {
                    Some(entities) => {
                        self.entities = entities.clone();
                        true
                    }
                    None => false,
                },
                13 => match result.as_array() {
                    Some(devices) => {
                        self.devices = devices.clone();
                        true
                    }
                    None => false,
                },
                _ => false,
            }
        };
        if !valid {
            self.catalog.warnings.push(format!("{key}_unavailable"));
        }
        self.catalog.entity_areas = self
            .entities
            .iter()
            .filter_map(|entity| {
                let id = entity["entity_id"].as_str()?;
                let area = entity["area_id"].as_str().or_else(|| {
                    let device_id = entity["device_id"].as_str()?;
                    self.devices
                        .iter()
                        .find(|device| device["id"].as_str() == Some(device_id))?["area_id"]
                        .as_str()
                });
                Some((id.to_owned(), area.map(str::to_owned)))
            })
            .collect();
        Some(self.catalog.clone())
    }
}

/// Export only capability attributes needed by the UI. Never forward arbitrary HA attributes.
pub fn safe_attributes(raw: &Value) -> Value {
    let mut result = serde_json::Map::new();
    for key in [
        "brightness",
        "color_temp_kelvin",
        "min_color_temp_kelvin",
        "max_color_temp_kelvin",
        "temperature",
        "current_temperature",
        "min_temp",
        "max_temp",
        "target_temp_step",
        "percentage",
        "percentage_step",
        "supported_features",
    ] {
        if let Some(value) = raw.get(key) {
            if value.is_number() || value.is_null() {
                result.insert(key.into(), value.clone());
            }
        }
    }
    for key in [
        "supported_color_modes",
        "hvac_modes",
        "fan_modes",
        "preset_modes",
    ] {
        if let Some(values) = raw.get(key).and_then(Value::as_array) {
            result.insert(
                key.into(),
                Value::Array(
                    values
                        .iter()
                        .filter_map(|value| value.as_str())
                        .filter(|value| value.len() <= 128)
                        .take(64)
                        .map(|value| Value::String(value.into()))
                        .collect(),
                ),
            );
        }
    }
    for key in ["fan_mode", "preset_mode", "temperature_unit"] {
        if let Some(value) = raw
            .get(key)
            .and_then(Value::as_str)
            .filter(|value| value.len() <= 128)
        {
            result.insert(key.into(), Value::String(value.into()));
        }
    }
    if let Some(rgb) = raw.get("rgb_color").and_then(Value::as_array) {
        if rgb.len() == 3
            && rgb
                .iter()
                .all(|value| value.as_u64().is_some_and(|v| v <= 255))
        {
            result.insert("rgb_color".into(), Value::Array(rgb.clone()));
        }
    }
    Value::Object(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn registry_mapping_prefers_entity_area_and_metadata_failures_do_not_remove_areas() {
        let mut builder = CatalogBuilder::default();
        builder.apply(
            &json!({"id":11,"success":true,"result":[{"area_id":"office","name":"Office"}]}),
        );
        builder.apply(&json!({"id":12,"success":true,"result":[{"entity_id":"light.one","device_id":"d1","area_id":null},{"entity_id":"light.two","device_id":"d1","area_id":"bedroom"}]}));
        let result = builder
            .apply(&json!({"id":13,"success":true,"result":[{"id":"d1","area_id":"office"}]}))
            .unwrap();
        assert_eq!(result.entity_areas["light.one"].as_deref(), Some("office"));
        assert_eq!(result.entity_areas["light.two"].as_deref(), Some("bedroom"));
        let result = builder
            .apply(&json!({"id":10,"success":false,"error":{"message":"private upstream detail"}}))
            .unwrap();
        assert_eq!(result.areas.len(), 1);
        assert!(!result.services_loaded);
        assert_eq!(result.warnings, vec!["services_unavailable"]);
        assert!(
            !serde_json::to_string(&result)
                .unwrap()
                .contains("private upstream detail")
        );
    }
    #[test]
    fn capability_export_never_includes_credentials_or_wrong_types() {
        let result = safe_attributes(
            &json!({"access_token":"secret", "entity_picture":"/api/camera?token=secret", "brightness":128,"temperature":"secret","supported_color_modes":["brightness"],"rgb_color":[1,2,3]}),
        );
        assert_eq!(result["brightness"], 128);
        assert_eq!(result["rgb_color"], json!([1, 2, 3]));
        assert!(!serde_json::to_string(&result).unwrap().contains("secret"));
    }
}
