//! MISS command/media framing, separate from the CS2 transport.
use super::cs2::{failure, malformed};
use crate::errors::AppError;
use chacha20::{
    ChaCha20,
    cipher::{KeyIvInit, StreamCipher},
};
use rand::RngCore;
use zeroize::Zeroizing;

pub fn hex_key(raw: &str) -> Result<[u8; 32], AppError> {
    if raw.len() != 64 || !raw.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(malformed());
    }
    let mut key = [0; 32];
    for (i, pair) in raw.as_bytes().as_chunks::<2>().0.iter().enumerate() {
        let s = std::str::from_utf8(pair).map_err(|_| malformed())?;
        key[i] = u8::from_str_radix(s, 16).map_err(|_| malformed())?;
    }
    Ok(key)
}
pub fn decrypt(key: &[u8; 32], input: &[u8]) -> Result<Zeroizing<Vec<u8>>, AppError> {
    if input.len() < 8 {
        return Err(malformed());
    }
    let mut iv = [0; 12];
    iv[4..].copy_from_slice(&input[..8]);
    let mut bytes = Zeroizing::new(input[8..].to_vec());
    ChaCha20::new(key.into(), (&iv).into()).apply_keystream(&mut bytes);
    Ok(bytes)
}
pub fn encrypted_command(key: &[u8; 32], command: u32, body: &[u8]) -> Vec<u8> {
    let mut iv = [0; 12];
    rand::rng().fill_bytes(&mut iv[4..]);
    let mut plain = Zeroizing::new(command.to_be_bytes().to_vec());
    plain.extend(body);
    ChaCha20::new(key.into(), (&iv).into()).apply_keystream(&mut plain);
    let mut out = 0x1001u32.to_be_bytes().to_vec();
    out.extend(&iv[4..]);
    out.extend(&*plain);
    out
}
pub fn auth_message(public: &str, sign: &str) -> Zeroizing<Vec<u8>> {
    #[derive(serde::Serialize)]
    struct Auth<'a> {
        public_key: &'a str,
        sign: &'a str,
        uuid: String,
        support_encrypt: u8,
    }
    let mut msg = Zeroizing::new(0x100u32.to_be_bytes().to_vec());
    let body = Zeroizing::new(
        serde_json::to_vec(&Auth {
            public_key: public,
            sign,
            uuid: uuid::Uuid::new_v4().to_string(),
            support_encrypt: 15,
        })
        .expect("valid JSON"),
    );
    msg.extend(&*body);
    msg
}
/// Structural diagnostics only; no header bytes or JSON values cross IPC.
#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthDiagnostics {
    pub messages: u32,
    pub message_kind: &'static str,
    pub command_byte_order: Option<&'static str>,
    pub body_bytes: usize,
    pub trailing_padding_bytes: usize,
    pub encrypted: bool,
    pub json_shape: &'static str,
    pub json_error_category: Option<&'static str>,
}
impl Default for AuthDiagnostics {
    fn default() -> Self {
        Self {
            messages: 0,
            message_kind: "none",
            command_byte_order: None,
            body_bytes: 0,
            trailing_padding_bytes: 0,
            encrypted: false,
            json_shape: "none",
            json_error_category: None,
        }
    }
}
fn auth_error(code: &'static str, message: &str) -> AppError {
    AppError::new(code, message, false)
}
pub fn authenticated(
    message: &[u8],
    key: &[u8; 32],
    diagnostic: &mut AuthDiagnostics,
) -> Result<bool, AppError> {
    diagnostic.messages = diagnostic.messages.saturating_add(1);
    diagnostic.encrypted = false;
    diagnostic.json_error_category = None;
    diagnostic.trailing_padding_bytes = 0;
    diagnostic.json_shape = "none";
    diagnostic.command_byte_order = None;
    diagnostic.body_bytes = message.len().saturating_sub(4);
    auth_reply(message, key, diagnostic, 0)
}
fn auth_reply(
    message: &[u8],
    key: &[u8; 32],
    diagnostic: &mut AuthDiagnostics,
    depth: u8,
) -> Result<bool, AppError> {
    if message.len() < 4 {
        diagnostic.message_kind = "short";
        return Err(auth_error(
            "MISS_AUTH_COMMAND_SHORT",
            "摄像头认证响应缺少命令头",
        ));
    }
    let word: [u8; 4] = message[..4].try_into().unwrap();
    let be = u32::from_be_bytes(word);
    let le = u32::from_le_bytes(word);
    // go2rtc reads camera response words as LE; micam documents BE. Limit
    // compatibility to recognized MISS identifiers, never arbitrary offsets.
    let command = if matches!(be, 0x101 | 0x1001) {
        diagnostic.command_byte_order = Some("be");
        be
    } else if matches!(le, 0x101 | 0x1001) {
        diagnostic.command_byte_order = Some("le");
        le
    } else {
        diagnostic.message_kind = "other";
        return Ok(false);
    };
    if command == 0x1001 {
        diagnostic.message_kind = "encrypted";
        diagnostic.encrypted = true;
        if depth != 0 || message.len() < 12 || message.len() > 32780 {
            return Err(auth_error(
                "MISS_AUTH_ENVELOPE_INVALID",
                "摄像头认证加密响应格式不匹配",
            ));
        }
        let plain = decrypt(key, &message[4..])
            .map_err(|_| auth_error("MISS_AUTH_ENVELOPE_INVALID", "无法解析摄像头认证加密响应"))?;
        return auth_reply(&plain, key, diagnostic, depth + 1);
    }
    diagnostic.message_kind = "auth_response";
    let body = &message[4..];
    diagnostic.body_bytes = body.len();
    if body.len() > 32768 {
        return Err(auth_error(
            "MISS_AUTH_RESPONSE_TOO_LARGE",
            "摄像头认证响应超过支持的长度",
        ));
    }
    let end = body
        .iter()
        .rposition(|b| *b != 0 && !b.is_ascii_whitespace())
        .map_or(0, |i| i + 1);
    diagnostic.trailing_padding_bytes = body.len() - end;
    let body = &body[..end];
    let start = body
        .iter()
        .position(|b| !b.is_ascii_whitespace())
        .unwrap_or(body.len());
    let body = &body[start..];
    let body = body.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(body);
    let start = body
        .iter()
        .position(|b| !b.is_ascii_whitespace())
        .unwrap_or(body.len());
    let body = &body[start..];
    diagnostic.json_shape = match body.first() {
        Some(b'{') => "object",
        Some(b'[') => "array",
        None => "empty",
        _ => "other",
    };
    #[derive(serde::Deserialize)]
    struct AuthResult {
        result: String,
    }
    let result: AuthResult = serde_json::from_slice(body).map_err(|e| {
        diagnostic.json_error_category = Some(match e.classify() {
            serde_json::error::Category::Io => "io",
            serde_json::error::Category::Syntax => "syntax",
            serde_json::error::Category::Data => "data",
            serde_json::error::Category::Eof => "eof",
        });
        auth_error(
            "MISS_AUTH_JSON_INVALID",
            "摄像头认证响应格式暂不兼容，请复制连接结果",
        )
    })?;
    if result.result != "success" {
        return Err(auth_error(
            "MISS_AUTH_REJECTED",
            "摄像头拒绝了视频授权，请重新连接",
        ));
    }
    Ok(true)
}

pub struct VideoPacket {
    pub codec: u32,
    pub sequence: u32,
    pub timestamp: u64,
    pub keyframe: bool,
    pub bytes: Zeroizing<Vec<u8>>,
}
/// Preserve codec configuration when it precedes the first keyframe in a
/// separate packet. Only parameter NALs are retained, not undecodable deltas.
pub fn parameter_sets(codec: u32, bytes: &[u8]) -> Zeroizing<Vec<u8>> {
    let mut starts = Vec::new();
    let mut i = 0;
    while i + 3 <= bytes.len() {
        let n = if bytes[i..].starts_with(&[0, 0, 0, 1]) {
            4
        } else if bytes[i..].starts_with(&[0, 0, 1]) {
            3
        } else {
            i += 1;
            continue;
        };
        starts.push((i, n));
        i += n;
    }
    let mut out = Zeroizing::new(Vec::new());
    for (index, &(start, prefix)) in starts.iter().enumerate() {
        let Some(&first) = bytes.get(start + prefix) else {
            continue;
        };
        let kind = if codec == 4 {
            first & 31
        } else {
            (first >> 1) & 63
        };
        if (codec == 4 && matches!(kind, 7 | 8)) || (codec == 5 && matches!(kind, 32..=34)) {
            let end = starts.get(index + 1).map_or(bytes.len(), |s| s.0);
            if out.len() + end - start <= 65536 {
                out.extend(&bytes[start..end]);
            }
        }
    }
    out
}
pub fn video(key: &[u8; 32], packet: &[u8]) -> Result<Option<VideoPacket>, AppError> {
    if packet.len() < 40 {
        return Err(malformed());
    }
    let field = |i| u32::from_le_bytes(packet[i..i + 4].try_into().unwrap());
    if field(0) as usize != packet.len() - 32 {
        return Err(malformed());
    }
    let codec = field(4);
    if !matches!(codec, 4 | 5) {
        return Ok(None);
    } // video-only milestone
    let bytes = decrypt(key, &packet[32..])?;
    if !bytes.starts_with(&[0, 0, 0, 1]) && !bytes.starts_with(&[0, 0, 1]) {
        return Err(failure(
            "MISS_VIDEO_INVALID",
            "摄像头视频解密或编码格式不匹配",
        ));
    }
    Ok(Some(VideoPacket {
        codec,
        sequence: field(8),
        keyframe: field(12) & 1 != 0,
        timestamp: u64::from_le_bytes(packet[16..24].try_into().unwrap()),
        bytes,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn nacl_box_precompute_matches_published_alice_bob_vector() {
        let a = x25519_dalek::StaticSecret::from(
            hex_key("77076d0a7318a57d3c16c17251b26645df4c2f87ebc0992ab177fba51db92c2a").unwrap(),
        );
        let b = x25519_dalek::PublicKey::from(
            hex_key("de9edb7d7b7dc1b4d35b61c2ece435373f8343c85b78674dadfc7e146f882b4f").unwrap(),
        );
        let shared = a.diffie_hellman(&b);
        let key = salsa20::hsalsa::<salsa20::cipher::consts::U10>(
            shared.as_bytes().into(),
            (&[0; 16]).into(),
        );
        assert_eq!(
            &key[..],
            &hex_key("1b27556473e985d462cd51197a9a46c76009549eac6474f206c4ee0844f68389").unwrap()
        );
        assert_ne!(&key[..], shared.as_bytes());
    }
    #[test]
    fn commands_use_nonce_prefix_and_auth_json_is_not_substring_matched() {
        let key = [9; 32];
        let one = encrypted_command(&key, 0x102, b"{}");
        let two = encrypted_command(&key, 0x102, b"{}");
        assert_ne!(one, two);
        assert_eq!(
            &*decrypt(&key, &one[4..]).unwrap(),
            &[0, 0, 1, 2, b'{', b'}']
        );
        assert!(decrypt(&key, b"short").is_err());
        let mut bad = 0x101u32.to_be_bytes().to_vec();
        bad.extend(br#"{"error":"result=success"}"#);
        assert!(authenticated(&bad, &key, &mut AuthDiagnostics::default()).is_err());
        assert!(video(&key, &[0; 39]).is_err());
    }
    #[test]
    fn media_is_little_endian_and_headers_survive_separate_parameter_packets() {
        let key = [5; 32];
        let cipher = encrypted_command(&key, 1, &[0x67, 9, 8]);
        let mut packet = vec![0; 32];
        packet[..4].copy_from_slice(&((cipher.len() - 4) as u32).to_le_bytes());
        packet[4..8].copy_from_slice(&4u32.to_le_bytes());
        packet[12] = 1;
        packet[16..24].copy_from_slice(&123u64.to_le_bytes());
        packet.extend(&cipher[4..]);
        let parsed = video(&key, &packet).unwrap().unwrap();
        assert!(parsed.keyframe);
        assert_eq!(parsed.timestamp, 123);
        assert_eq!(parsed.codec, 4);
        assert_eq!(
            &*parameter_sets(4, &parsed.bytes),
            &[0, 0, 0, 1, 0x67, 9, 8]
        );
        packet[0] = 0;
        assert!(video(&key, &packet).is_err());
        assert!(parameter_sets(4, &[0, 0, 1, 0x41, 5, 6]).is_empty());
    }
    #[test]
    fn auth_accepts_known_byte_orders_and_zero_terminated_json_but_not_forged_success() {
        let key = [11; 32];
        for header in [0x101u32.to_be_bytes(), 0x101u32.to_le_bytes()] {
            let mut packet = header.to_vec();
            packet.extend(b" \xef\xbb\xbf {\"result\" : \"success\", \"channel\":\"private-channel-fixture\"}\0\0 \r\n");
            let mut diag = AuthDiagnostics::default();
            assert!(authenticated(&packet, &key, &mut diag).unwrap());
            assert_eq!(diag.message_kind, "auth_response");
            assert_eq!(diag.trailing_padding_bytes, 5);
            assert!(
                !serde_json::to_string(&diag)
                    .unwrap()
                    .contains("private-channel")
            );
        }
        for body in [
            &b"{\"result\":\"success\"}\0not-padding"[..],
            &b"{\"result\":\"failed\",\"message\":\"success\"}"[..],
            &b"{\"result\":\"failed\",\"result\":\"success\"}"[..],
            &b"{\"message\":\"result=success\"}"[..],
            &b"{\"result\":\"success\"}{\"result\":\"failed\"}"[..],
            &b"{\"result\":\"success"[..],
        ] {
            let mut packet = 0x101u32.to_be_bytes().to_vec();
            packet.extend(body);
            let err = authenticated(&packet, &key, &mut AuthDiagnostics::default()).unwrap_err();
            assert!(!err.retryable);
            assert_ne!(err.code, "CS2_INVALID_PACKET");
        }
    }
    #[test]
    fn encrypted_auth_is_bounded_and_reports_shape_without_decrypted_values() {
        let key = [12; 32];
        let mut diag = AuthDiagnostics::default();
        let packet = encrypted_command(
            &key,
            0x101,
            b"{\"result\":\"success\",\"channel\":\"private-channel\"}\0",
        );
        assert!(authenticated(&packet, &key, &mut diag).unwrap());
        assert!(diag.encrypted);
        assert_eq!(diag.trailing_padding_bytes, 1);
        assert!(
            !serde_json::to_string(&diag)
                .unwrap()
                .contains("private-channel")
        );
        assert_eq!(
            authenticated(&[0, 0], &key, &mut diag).unwrap_err().code,
            "MISS_AUTH_COMMAND_SHORT"
        );
        let mut huge = 0x101u32.to_be_bytes().to_vec();
        huge.extend(vec![b' '; 32769]);
        assert_eq!(
            authenticated(&huge, &key, &mut diag).unwrap_err().code,
            "MISS_AUTH_RESPONSE_TOO_LARGE"
        );
        assert!(!authenticated(&0x110u32.to_be_bytes(), &key, &mut diag).unwrap());
        let nested = encrypted_command(&key, 0x1001, &packet[4..]);
        assert_eq!(
            authenticated(&nested, &key, &mut diag).unwrap_err().code,
            "MISS_AUTH_ENVELOPE_INVALID"
        );
    }
}
