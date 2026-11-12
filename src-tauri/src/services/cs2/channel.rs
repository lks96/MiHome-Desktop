use super::{failure, malformed};
use crate::errors::AppError;
use std::collections::{BTreeMap, VecDeque};

pub const MAX_MESSAGE: usize = 4 * 1024 * 1024;
const WINDOW: u16 = 256;
/// One reliable receive stream; sequence arithmetic is modulo 2^16.
#[derive(Default)]
pub struct Receiver {
    next: u16,
    pending: BTreeMap<u16, Vec<u8>>,
    stream: VecDeque<u8>,
}
impl Receiver {
    pub fn push(&mut self, sequence: u16, payload: &[u8]) -> Result<Vec<Vec<u8>>, AppError> {
        if payload.len() > 2048 {
            return Err(malformed());
        }
        let distance = sequence.wrapping_sub(self.next);
        if distance >= 0x8000 {
            return Ok(Vec::new());
        } // old fragment; caller still ACKs
        if distance >= WINDOW {
            return Err(failure("CS2_SEQUENCE_GAP", "视频数据丢包过多，请重新连接"));
        }
        self.pending
            .entry(sequence)
            .or_insert_with(|| payload.to_vec());
        while let Some(bytes) = self.pending.remove(&self.next) {
            if self.stream.len() + bytes.len() > MAX_MESSAGE + 4 {
                return Err(malformed());
            }
            self.stream.extend(bytes);
            self.next = self.next.wrapping_add(1);
        }
        let mut output = Vec::new();
        while self.stream.len() >= 4 {
            let mut len = [0; 4];
            for (i, b) in self.stream.iter().take(4).enumerate() {
                len[i] = *b;
            }
            let len = u32::from_be_bytes(len) as usize;
            if len == 0 || len > MAX_MESSAGE {
                return Err(malformed());
            }
            if self.stream.len() < len + 4 {
                break;
            }
            self.stream.drain(..4);
            output.push(self.stream.drain(..len).collect());
            if output.len() > 32 {
                return Err(malformed());
            }
        }
        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reorder_duplicate_length_split_and_wrap() {
        let mut r = Receiver::default();
        r.next = u16::MAX;
        assert!(r.push(0, &[0, 3, b'a']).unwrap().is_empty());
        assert!(r.push(u16::MAX, &[0, 0]).unwrap().is_empty());
        assert_eq!(r.push(1, b"bc").unwrap(), vec![b"abc".to_vec()]);
        assert!(r.push(0, &[0, 3, b'a']).unwrap().is_empty());
        assert!(r.push(258, b"x").is_err());
    }
    #[test]
    fn hostile_size_fails_without_allocation() {
        assert!(
            Receiver::default()
                .push(0, &u32::MAX.to_be_bytes())
                .is_err()
        );
        assert!(Receiver::default().push(0, &[0, 0, 0, 0]).is_err());
    }
}
