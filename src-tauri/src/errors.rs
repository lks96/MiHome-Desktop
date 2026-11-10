use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppError {
    pub code: &'static str,
    pub message: String,
    pub retryable: bool,
}
impl AppError {
    pub fn new(code: &'static str, message: &str, retryable: bool) -> Self {
        Self {
            code,
            message: message.into(),
            retryable,
        }
    }

    pub fn invalid_input(message: &str) -> Self {
        Self {
            code: "INVALID_INPUT",
            message: message.into(),
            retryable: false,
        }
    }
    pub fn database(operation: &'static str) -> Self {
        // Do not include SQL arguments or serialized settings in logs.
        tracing::error!(operation, "Local settings database operation failed");
        Self {
            code: "DATABASE_ERROR",
            message: "本地配置读写失败，请重试或查看应用日志".into(),
            retryable: true,
        }
    }
}
#[derive(Serialize)]
#[serde(untagged)]
pub enum ApiResult<T> {
    Success { ok: bool, data: T },
    Failure { ok: bool, error: AppError },
}
impl<T> From<Result<T, AppError>> for ApiResult<T> {
    fn from(result: Result<T, AppError>) -> Self {
        match result {
            Ok(data) => Self::Success { ok: true, data },
            Err(error) => Self::Failure { ok: false, error },
        }
    }
}
