use super::malformed;
use crate::errors::AppError;
use std::net::{Ipv4Addr, SocketAddrV4};

pub const LIST: u8 = 0x67;
pub const HELLO: u8 = 0x70;
pub const HELLO_ACK: u8 = 0x71;
pub const PORT: u8 = 0x72;
pub const PORT_ACK: u8 = 0x73;
pub const REQUEST: u8 = 0x80;
pub const CANDIDATE: u8 = 0x82;
pub const REGISTER: u8 = 0x83;
pub const READY: u8 = 0x84;
pub const LINK: u8 = 0x28;
pub const DATA: u8 = 0xd0;
pub const ACK: u8 = 0xd1;
pub const ALIVE: u8 = 0xe0;
pub const ALIVE_ACK: u8 = 0xe1;
pub const CLOSE: u8 = 0xf0;

pub fn frame(kind: u8, body: &[u8]) -> Vec<u8> {
    assert!(body.len() <= u16::MAX as usize);
    let mut out = Vec::with_capacity(body.len() + 4);
    out.extend([0xf1, kind]);
    out.extend((body.len() as u16).to_be_bytes());
    out.extend(body);
    out
}
pub fn parse(input: &[u8]) -> Option<(u8, &[u8])> {
    if input.len() < 4 || input[0] != 0xf1 {
        return None;
    }
    let length = u16::from_be_bytes([input[2], input[3]]) as usize;
    (input.len() == length + 4).then_some((input[1], &input[4..]))
}

pub fn public_ipv4(ip: Ipv4Addr) -> bool {
    let a = ip.octets();
    !ip.is_private()
        && !ip.is_loopback()
        && !ip.is_link_local()
        && !ip.is_broadcast()
        && !ip.is_documentation()
        && a[0] != 0
        && a[0] < 224
        && !(a[0] == 100 && (64..=127).contains(&a[1]))
        && !(a[0] == 198 && (18..=19).contains(&a[1]))
}
pub fn address(input: &[u8]) -> Result<SocketAddrV4, AppError> {
    if input.len() < 16 || input[..2] != [0, 2] {
        return Err(malformed());
    }
    let port = u16::from_le_bytes([input[2], input[3]]);
    if port == 0 {
        return Err(malformed());
    }
    Ok(SocketAddrV4::new(
        Ipv4Addr::new(input[7], input[6], input[5], input[4]),
        port,
    ))
}
pub fn address_bytes(addr: SocketAddrV4) -> [u8; 16] {
    let mut out = [0; 16];
    out[..2].copy_from_slice(&[0, 2]);
    out[2..4].copy_from_slice(&addr.port().to_le_bytes());
    let mut ip = addr.ip().octets();
    ip.reverse();
    out[4..8].copy_from_slice(&ip);
    out
}
pub fn peer_id(value: &str) -> Result<[u8; 20], AppError> {
    let parts: Vec<_> = value.split('-').collect();
    if parts.len() != 3
        || [parts[0], parts[2]]
            .iter()
            .any(|v| v.is_empty() || v.len() > 8 || !v.bytes().all(|b| b.is_ascii_alphanumeric()))
        || parts[1].is_empty()
        || !parts[1].bytes().all(|b| b.is_ascii_digit())
    {
        return Err(malformed());
    }
    let serial = parts[1].parse::<u32>().map_err(|_| malformed())?;
    let mut out = [0; 20];
    out[..parts[0].len()].copy_from_slice(parts[0].as_bytes());
    out[8..12].copy_from_slice(&serial.to_be_bytes());
    out[12..12 + parts[2].len()].copy_from_slice(parts[2].as_bytes());
    Ok(out)
}

// Protocol substitution table, not a credential. Encoding is A..P nibbles with
// a running XOR initialized to 0x39. No server addresses are compiled in.
const INIT_TABLE: [u8; 54] = [
    0x49, 0x59, 0x43, 0x3d, 0xb5, 0xbf, 0x6d, 0xa3, 0x47, 0x53, 0x4f, 0x61, 0x65, 0xe3, 0x71, 0xe9,
    0x67, 0x7f, 0x02, 0x03, 0x0b, 0xad, 0xb3, 0x89, 0x2b, 0x2f, 0x35, 0xc1, 0x6b, 0x8b, 0x95, 0x97,
    0x11, 0xe5, 0xa7, 0x0d, 0xef, 0xf1, 0x05, 0x07, 0x83, 0xfb, 0x9d, 0x3b, 0xc5, 0xc7, 0x13, 0x17,
    0x1d, 0x1f, 0x25, 0x29, 0xd3, 0xdf,
];
pub fn servers(encoded: &str) -> Result<Vec<Ipv4Addr>, AppError> {
    let fail = || super::failure("CS2_INIT_DECODE_FAILED", "无法解析摄像头远程连接参数");
    let input = encoded.split(':').next().unwrap_or("").as_bytes();
    if input.is_empty()
        || input.len() > 4096
        || !input.len().is_multiple_of(2)
        || !input.iter().all(|b| (b'A'..=b'P').contains(b))
    {
        return Err(fail());
    }
    let mut decoded = zeroize::Zeroizing::new(Vec::with_capacity(input.len() / 2));
    let mut running = 0x39;
    for (i, pair) in input.as_chunks::<2>().0.iter().enumerate() {
        let byte = running ^ INIT_TABLE[i % 54] ^ ((pair[0] - b'A') * 16 + (pair[1] - b'A'));
        decoded.push(byte);
        running ^= byte;
    }
    let raw = std::str::from_utf8(&decoded)
        .map_err(|_| fail())?
        .trim_end_matches('\0');
    let mut output = Vec::new();
    for item in raw.split(',').filter(|s| !s.is_empty()) {
        let ip: Ipv4Addr = item.parse().map_err(|_| fail())?;
        if !public_ipv4(ip) || output.len() >= 16 {
            return Err(fail());
        }
        if !output.contains(&ip) {
            output.push(ip);
        }
    }
    if output.is_empty() {
        return Err(fail());
    }
    Ok(output)
}

pub fn is_link_request(input: &[u8]) -> bool {
    (88..=4092).contains(&input.len()) && input[..2] == [0xa1, 1]
}
pub fn linking_reply(input: &[u8], local: SocketAddrV4) -> Result<Vec<u8>, AppError> {
    if !is_link_request(input) {
        return Err(malformed());
    }
    let mut reply = input.to_vec();
    reply[1] = 2;
    reply[56..60].copy_from_slice(&[0xfe, 0xff, 0xff, 0xff]);
    for i in 0..4 {
        reply[60 + i] = input[7 - i];
    }
    reply[64..80].copy_from_slice(&address_bytes(local));
    reply[80..88].copy_from_slice(&input[40..48]);
    Ok(reply)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wire_endianness_and_truncation() {
        let endpoint = SocketAddrV4::new(Ipv4Addr::new(1, 2, 3, 4), 0x1234);
        assert_eq!(
            &address_bytes(endpoint)[..8],
            &[0, 2, 0x34, 0x12, 4, 3, 2, 1]
        );
        assert_eq!(address(&address_bytes(endpoint)).unwrap(), endpoint);
        for n in 0..16 {
            assert!(address(&address_bytes(endpoint)[..n]).is_err());
        }
        let packet = frame(DATA, b"abc");
        assert_eq!(packet, [0xf1, 0xd0, 0, 3, b'a', b'b', b'c']);
        assert_eq!(parse(&packet), Some((DATA, &b"abc"[..])));
        for n in 0..packet.len() {
            assert!(parse(&packet[..n]).is_none());
        }
        assert!(parse(&[0xf1, 0xd0, 0, 0, 1]).is_none());
        assert!(peer_id("A-4294967296-B").is_err());
        assert!(peer_id("TOO_LONG_PREFIX-1-B").is_err());
        assert_eq!(
            &peer_id("TEST-16909060-CHECK").unwrap()[8..12],
            &[1, 2, 3, 4]
        );
    }
    #[test]
    fn init_rejects_invalid_and_private_servers() {
        fn encode(raw: &[u8]) -> String {
            let mut running = 0x39;
            let mut out = String::new();
            for (i, &b) in raw.iter().enumerate() {
                let v = b ^ running ^ INIT_TABLE[i % 54];
                running ^= b;
                out.push((b'A' + (v >> 4)) as char);
                out.push((b'A' + (v & 15)) as char);
            }
            out
        }
        assert_eq!(servers(&encode(b"1.2.3.4,8.8.8.8,")).unwrap().len(), 2);
        for raw in [
            "127.0.0.1,",
            "10.1.1.1,",
            "0.0.0.0,",
            "224.1.1.1,",
            "1.2.3.4:80,",
        ] {
            assert!(servers(&encode(raw.as_bytes())).is_err());
        }
        for bad in ["", "A", "QQ", "ABCDZ!"] {
            assert!(servers(bad).is_err());
        }
    }
    #[test]
    fn session_link_preserves_identity_and_rewrites_addresses() {
        let mut input = [0; 88];
        input[..2].copy_from_slice(&[0xa1, 1]);
        input[4..8].copy_from_slice(&[1, 2, 3, 4]);
        input[40..48].copy_from_slice(&[0, 2, 5, 6, 7, 8, 9, 10]);
        let out = linking_reply(&input, SocketAddrV4::new(Ipv4Addr::LOCALHOST, 4321)).unwrap();
        assert_eq!(&out[4..56], &input[4..56]);
        assert_eq!(&out[56..64], &[0xfe, 0xff, 0xff, 0xff, 4, 3, 2, 1]);
        assert_eq!(&out[80..88], &input[40..48]);
        for n in 0..88 {
            assert!(linking_reply(&input[..n], SocketAddrV4::new(Ipv4Addr::LOCALHOST, 1)).is_err());
        }
        let mut confirmation = input;
        confirmation[1] = 2;
        assert!(!is_link_request(&confirmation));
        // Calling the request rewriter on this valid non-request reproduces
        // the old transport failure. The actor must filter it before rewriting.
        assert_eq!(
            linking_reply(&confirmation, SocketAddrV4::new(Ipv4Addr::LOCALHOST, 1))
                .unwrap_err()
                .code,
            "CS2_INVALID_PACKET"
        );
        let mut extended = input.to_vec();
        extended.extend([9, 8, 7, 6]);
        let reply = linking_reply(&extended, SocketAddrV4::new(Ipv4Addr::LOCALHOST, 1)).unwrap();
        assert_eq!(&reply[88..], &[9, 8, 7, 6]);
    }
}
