use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    #[default]
    Light,
    Dark,
    System,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AppSettings {
    pub theme: Theme,
    pub request_timeout: u32,
    pub auto_connect: bool,
    pub auto_reconnect: bool,
    #[serde(default)]
    pub close_to_tray: bool,
    #[serde(default)]
    pub auto_start: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            theme: Theme::Light,
            request_timeout: 10,
            auto_connect: true,
            auto_reconnect: true,
            close_to_tray: false,
            auto_start: false,
        }
    }
}
impl AppSettings {
    pub fn validate(&self) -> Result<(), crate::errors::AppError> {
        if !(1..=120).contains(&self.request_timeout) {
            return Err(crate::errors::AppError::invalid_input(
                "请求超时必须在 1 至 120 秒之间",
            ));
        }
        Ok(())
    }
}
