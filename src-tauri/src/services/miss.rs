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
pub fn authenticated(message: &[u8]) -> Result<bool, AppError> {
    if message.len() < 4 {
        return Err(malformed());
    }
    if u32::from_be_bytes(message[..4].try_into().unwrap()) != 0x101 {
        return Ok(false);
    }
    let result: serde_json::Value =
        serde_json::from_slice(&message[4..]).map_err(|_| malformed())?;
    if result["result"].as_str() != Some("success") {
        return Err(failure(
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
        assert!(authenticated(&bad).is_err());
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
}
