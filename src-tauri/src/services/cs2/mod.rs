//! Independently implemented CS2 wire protocol. See docs/CS2_PROTOCOL.md.
pub mod channel;
pub mod protocol;
pub mod relay;

use crate::errors::AppError;
pub fn failure(code: &'static str, message: &str) -> AppError {
    AppError::new(code, message, true)
}
pub fn malformed() -> AppError {
    failure("CS2_INVALID_PACKET", "摄像头连接返回了无法识别的数据")
}
