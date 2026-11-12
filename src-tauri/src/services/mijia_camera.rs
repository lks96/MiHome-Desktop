use crate::errors::AppError;
use rand::RngCore;
use serde::Serialize;
use serde_json::Value;
use x25519_dalek::{PublicKey, StaticSecret};
use zeroize::Zeroizing;

pub const TARGET_MODEL: &str = "chuangmi.camera.039a01";

/// Only this allowlisted report crosses IPC. Never serialize the cloud response.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CameraProbeReport {
    pub model: &'static str,
    pub transport: &'static str,
    pub cloud_authorization_granted: bool,
    pub remote_playback_verified: bool,
    pub observed_at: u64,
}

pub fn challenge_public_key() -> String {
    let mut seed = Zeroizing::new([0u8; 32]);
    rand::rng().fill_bytes(&mut *seed);
    let private = StaticSecret::from(*seed);
    let public = PublicKey::from(&private);
    // A probe does not open a stream. The ephemeral private key is discarded.
    public
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

pub fn report(data: &Value, observed_at: u64) -> Result<CameraProbeReport, AppError> {
    let transport = match data["vendor"]["vendor"].as_u64() {
        Some(1) => "tutk",
        Some(3) => "agora",
        Some(4) => "cs2",
        Some(6) => "mtp",
        _ => {
            return Err(AppError::new(
                "CAMERA_PROTOCOL_UNKNOWN",
                "小米返回了尚未识别的视频协议，需要继续适配",
                false,
            ));
        }
    };
    let key_ok = data["public_key"]
        .as_str()
        .is_some_and(|key| key.len() == 64 && key.bytes().all(|b| b.is_ascii_hexdigit()));
    let sign_ok = data["sign"]
        .as_str()
        .is_some_and(|sign| !sign.is_empty() && sign.len() <= 4096);
    if !key_ok || !sign_ok {
        return Err(AppError::new(
            "CAMERA_AUTH_INCOMPLETE",
            "小米返回的视频授权信息不完整，尚不能建立视频通道",
            false,
        ));
    }
    Ok(CameraProbeReport {
        model: TARGET_MODEL,
        transport,
        cloud_authorization_granted: true,
        remote_playback_verified: false,
        observed_at,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn camera_reports_do_not_disclose_keys_tokens_or_addresses_or_claim_playback() {
        let input = json!({"vendor":{"vendor":4,"vendor_params":{"p2p_id":"private-uid","password":"private-password","server":"private-relay"}},"public_key":"ab".repeat(32),"sign":"private-signature","token":"private-token"});
        let value = report(&input, 123).unwrap();
        let public = serde_json::to_value(value).unwrap();
        assert_eq!(public["transport"], "cs2");
        assert_eq!(public["cloudAuthorizationGranted"], true);
        assert_eq!(public["remotePlaybackVerified"], false);
        assert!(!public.to_string().contains("private-"));
        assert_eq!(public.as_object().unwrap().len(), 5);
    }
    #[test]
    fn malformed_or_unknown_camera_grants_are_not_reported_as_authorized() {
        for data in [
            json!({}),
            json!({"vendor":{"vendor":99},"public_key":"ab".repeat(32),"sign":"fixture"}),
            json!({"vendor":{"vendor":4},"public_key":"bad","sign":"fixture"}),
            json!({"vendor":{"vendor":4},"public_key":"ab".repeat(32),"sign":""}),
        ] {
            assert!(report(&data, 0).is_err());
        }
    }
    #[test]
    fn probe_keys_are_fresh_curve25519_public_keys() {
        let first = challenge_public_key();
        assert_eq!(first.len(), 64);
        assert!(first.bytes().all(|b| b.is_ascii_hexdigit()));
        assert_ne!(first, challenge_public_key());
    }
}
