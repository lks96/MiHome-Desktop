use crate::errors::AppError;
use rand::RngCore;
use serde::Serialize;
use serde_json::Value;
use x25519_dalek::{PublicKey, StaticSecret};
use zeroize::Zeroizing;

pub const TARGET_MODEL: &str = "chuangmi.camera.039a01";

/// A playback grant is deliberately neither Debug nor Serialize. It never crosses IPC.
pub struct RemoteGrant {
    pub peer: Zeroizing<[u8; 20]>,
    pub servers: Vec<std::net::Ipv4Addr>,
    pub public: String,
    pub sign: Zeroizing<String>,
    pub key: Zeroizing<[u8; 32]>,
}
pub struct Challenge {
    private: StaticSecret,
    pub public: String,
}
impl Default for Challenge {
    fn default() -> Self {
        let mut seed = Zeroizing::new([0; 32]);
        rand::rng().fill_bytes(&mut *seed);
        let private = StaticSecret::from(*seed);
        let public = PublicKey::from(&private)
            .as_bytes()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        Self { private, public }
    }
}
pub fn remote_grant(data: &Value, challenge: Challenge) -> Result<RemoteGrant, AppError> {
    use super::{cs2, miss};
    let incomplete = || {
        AppError::new(
            "CAMERA_PROVISIONING_INCOMPLETE",
            "小米未返回完整的远程连接参数，请重新连接",
            true,
        )
    };
    report(data, 0)?;
    if data["vendor"]["vendor"].as_u64() != Some(4) {
        return Err(AppError::new(
            "CAMERA_VENDOR_UNSUPPORTED",
            "当前远程播放器仅适配 CS2 摄像头",
            false,
        ));
    }
    let params = &data["vendor"]["vendor_params"];
    let peer = cs2::protocol::peer_id(params["p2p_id"].as_str().ok_or_else(incomplete)?)?;
    let servers = cs2::protocol::servers(params["init_string"].as_str().ok_or_else(incomplete)?)?;
    let public = miss::hex_key(data["public_key"].as_str().ok_or_else(incomplete)?)?;
    let shared = challenge.private.diffie_hellman(&PublicKey::from(public));
    if !shared.was_contributory() {
        return Err(incomplete());
    }
    let secret = Zeroizing::new(shared.to_bytes());
    let key = salsa20::hsalsa::<salsa20::cipher::consts::U10>((&*secret).into(), (&[0; 16]).into());
    Ok(RemoteGrant {
        peer: Zeroizing::new(peer),
        servers,
        public: challenge.public,
        sign: Zeroizing::new(data["sign"].as_str().ok_or_else(incomplete)?.to_owned()),
        key: Zeroizing::new(key.into()),
    })
}

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
    fn provisioning_keeps_the_matching_ephemeral_key_and_rejects_noncontributory_keys() {
        let challenge = Challenge::default();
        let app_public = super::super::miss::hex_key(&challenge.public).unwrap();
        let device_private = StaticSecret::from([7; 32]);
        let device_public = PublicKey::from(&device_private)
            .as_bytes()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        let data = json!({"vendor":{"vendor":4,"vendor_params":{"p2p_id":"TEST-42-CHECK","init_string":"EBHPFHAHLMJIHOJM"}},"public_key":device_public,"sign":"fixture"});
        let expected = device_private.diffie_hellman(&PublicKey::from(app_public));
        let expected = salsa20::hsalsa::<salsa20::cipher::consts::U10>(
            expected.as_bytes().into(),
            (&[0; 16]).into(),
        );
        let grant = remote_grant(&data, challenge).unwrap();
        assert_eq!(&*grant.key, &expected[..]);
        assert_eq!(grant.servers, [std::net::Ipv4Addr::new(1, 2, 3, 4)]);
        let mut bad = data;
        bad["public_key"] = json!("00".repeat(32));
        assert!(remote_grant(&bad, Challenge::default()).is_err());
    }
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
