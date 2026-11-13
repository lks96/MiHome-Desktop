//! Minimal MPEG-TS input for FFmpeg. Camera access-unit timestamps must survive
//! stdin: an un-timed raw stream with `-r 20` starves playback for 10fps cameras.
use std::collections::VecDeque;

#[derive(Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TimingDiagnostics {
    pub timestamp_samples: u64,
    pub timestamp_fallbacks: u64,
    pub source_fps: Option<f64>,
    pub receive_fps: Option<f64>,
    pub largest_arrival_gap_ms: u64,
    pub encoder_write_max_ms: u64,
}
pub struct FrameClock {
    previous_source: Option<u64>,
    previous_arrival: Option<u64>,
    intervals: VecDeque<u64>,
    elapsed: u64,
    arrivals: VecDeque<u64>,
    pub diagnostics: TimingDiagnostics,
}
impl Default for FrameClock {
    fn default() -> Self {
        Self {
            previous_source: None,
            previous_arrival: None,
            intervals: VecDeque::new(),
            elapsed: 1000,
            arrivals: VecDeque::new(),
            diagnostics: TimingDiagnostics::default(),
        }
    }
}
impl FrameClock {
    pub fn timestamp(&mut self, source_ms: u64, arrival_ms: u64) -> u64 {
        let source_delta = self
            .previous_source
            .and_then(|p| source_ms.checked_sub(p))
            .filter(|d| (5..=2000).contains(d));
        let arrival_delta = self
            .previous_arrival
            .and_then(|p| arrival_ms.checked_sub(p));
        if let Some(gap) = arrival_delta {
            self.diagnostics.largest_arrival_gap_ms =
                self.diagnostics.largest_arrival_gap_ms.max(gap);
        }
        if self.previous_source.is_some() {
            let step = if let Some(delta) = source_delta {
                self.diagnostics.timestamp_samples += 1;
                self.intervals.push_back(delta);
                if self.intervals.len() > 30 {
                    self.intervals.pop_front();
                }
                delta
            } else {
                self.diagnostics.timestamp_fallbacks += 1;
                if !self.intervals.is_empty() {
                    self.intervals.iter().sum::<u64>() / self.intervals.len() as u64
                } else {
                    // With missing timestamps, use measured receive cadence
                    // across a window, not tiny inter-arrival gaps in a burst.
                    self.diagnostics
                        .receive_fps
                        .filter(|fps| (2.0..=120.0).contains(fps))
                        .map(|fps| (1000.0 / fps).round() as u64)
                        .or_else(|| arrival_delta.filter(|d| (5..=200).contains(d)))
                        .unwrap_or(50)
                }
            };
            self.elapsed = self.elapsed.saturating_add(step);
        }
        self.previous_source = Some(source_ms);
        self.previous_arrival = Some(arrival_ms);
        self.arrivals.push_back(arrival_ms);
        if self.arrivals.len() > 600 {
            self.arrivals.pop_front();
        }
        while self.arrivals.len() > 2
            && self
                .arrivals
                .front()
                .is_some_and(|t| arrival_ms.saturating_sub(*t) > 5000)
        {
            self.arrivals.pop_front();
        }
        self.diagnostics.source_fps = if self.intervals.is_empty() {
            None
        } else {
            Some(1000.0 * self.intervals.len() as f64 / self.intervals.iter().sum::<u64>() as f64)
        };
        self.diagnostics.receive_fps = self.arrivals.front().and_then(|first| {
            let span = arrival_ms.saturating_sub(*first);
            (span >= 500).then(|| 1000.0 * (self.arrivals.len() - 1) as f64 / span as f64)
        });
        self.elapsed.saturating_mul(90)
    }
}

/// ISO/IEC 13818-1 framing. One video PES per Annex-B access unit, with PTS and
/// PCR, PAT/PMT at random-access points. No vendor code or media data is stored.
pub struct TransportMuxer {
    codec: u32,
    pat_seq: u8,
    pmt_seq: u8,
    video_seq: u8,
    started: bool,
}
impl TransportMuxer {
    pub fn new(codec: u32) -> Self {
        Self {
            codec,
            pat_seq: 0,
            pmt_seq: 0,
            video_seq: 0,
            started: false,
        }
    }
    pub fn access_unit(&mut self, data: &[u8], pts: u64, keyframe: bool) -> Vec<u8> {
        let mut out = Vec::with_capacity(data.len() + 1024);
        if !self.started || keyframe {
            let pat = [0, 0xb0, 13, 0, 1, 0xc1, 0, 0, 0, 1, 0xf0, 0];
            section(&mut out, 0, &mut self.pat_seq, &pat);
            let pmt = [
                2,
                0xb0,
                18,
                0,
                1,
                0xc1,
                0,
                0,
                0xe1,
                0,
                0xf0,
                0,
                if self.codec == 4 { 0x1b } else { 0x24 },
                0xe1,
                0,
                0xf0,
                0,
            ];
            section(&mut out, 0x1000, &mut self.pmt_seq, &pmt);
            self.started = true;
        }
        let mut pes = Vec::with_capacity(data.len() + 14);
        pes.extend([0, 0, 1, 0xe0, 0, 0, 0x80, 0x80, 5]);
        let pts = pts & ((1u64 << 33) - 1);
        pes.extend([
            0x21 | (((pts >> 30) as u8 & 7) << 1),
            (pts >> 22) as u8,
            (((pts >> 15) as u8 & 0x7f) << 1) | 1,
            (pts >> 7) as u8,
            ((pts as u8 & 0x7f) << 1) | 1,
        ]);
        pes.extend(data);
        packets(
            &mut out,
            0x100,
            &mut self.video_seq,
            &pes,
            Some(pts.wrapping_sub(9000) & ((1u64 << 33) - 1)),
            keyframe,
        );
        out
    }
}
fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xffffffffu32;
    for byte in bytes {
        crc ^= (*byte as u32) << 24;
        for _ in 0..8 {
            crc = if crc & 0x80000000 != 0 {
                (crc << 1) ^ 0x04c11db7
            } else {
                crc << 1
            };
        }
    }
    crc
}
fn section(out: &mut Vec<u8>, pid: u16, sequence: &mut u8, section: &[u8]) {
    let mut data = vec![0];
    data.extend(section);
    data.extend(crc32(section).to_be_bytes());
    packets(out, pid, sequence, &data, None, false);
}
fn packets(
    out: &mut Vec<u8>,
    pid: u16,
    sequence: &mut u8,
    bytes: &[u8],
    pcr: Option<u64>,
    keyframe: bool,
) {
    let mut offset = 0;
    while offset < bytes.len() {
        let first = offset == 0;
        let minimum = if first && pcr.is_some() { 8 } else { 0 };
        let count = (184 - minimum).min(bytes.len() - offset);
        let adaptation = 184 - count;
        let mut packet = [0xff; 188];
        packet[0] = 0x47;
        packet[1] = ((pid >> 8) as u8) | if first { 0x40 } else { 0 };
        packet[2] = pid as u8;
        packet[3] = (if adaptation > 0 { 0x30 } else { 0x10 }) | (*sequence & 15);
        *sequence = sequence.wrapping_add(1) & 15;
        if adaptation > 0 {
            packet[4] = (adaptation - 1) as u8;
            if adaptation >= 2 {
                packet[5] = if first && pcr.is_some() {
                    0x10 | if keyframe { 0x40 } else { 0 }
                } else {
                    0
                };
            }
            if first {
                if let Some(base) = pcr {
                    packet[6] = (base >> 25) as u8;
                    packet[7] = (base >> 17) as u8;
                    packet[8] = (base >> 9) as u8;
                    packet[9] = (base >> 1) as u8;
                    packet[10] = ((base as u8 & 1) << 7) | 0x7e;
                    packet[11] = 0;
                }
            }
        }
        packet[4 + adaptation..].copy_from_slice(&bytes[offset..offset + count]);
        out.extend(packet);
        offset += count;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ten_fps_timestamps_are_not_compressed_into_twenty_fps() {
        let mut clock = FrameClock::default();
        let first = clock.timestamp(10000, 0);
        let mut last = first;
        for i in 1..40 {
            last = clock.timestamp(10000 + i * 100, i * 100);
        }
        assert_eq!(last - first, 3900 * 90);
        assert_eq!(clock.diagnostics.source_fps, Some(10.0));
        assert_eq!(clock.diagnostics.receive_fps, Some(10.0));
        let repeated = clock.timestamp(13900, 4000);
        assert_eq!(repeated - last, 100 * 90);
        let backwards = clock.timestamp(13000, 4100);
        assert_eq!(backwards - repeated, 100 * 90);
        assert_eq!(clock.diagnostics.timestamp_fallbacks, 2);
    }
    #[test]
    fn transport_sections_crc_pcr_continuity_and_wrap() {
        assert_eq!(crc32(b"123456789"), 0x0376e6e7);
        let mut mux = TransportMuxer::new(5);
        let data = mux.access_unit(&vec![0; 1000], 90000, true);
        assert_eq!(data.len() % 188, 0);
        assert!(data.chunks_exact(188).all(|p| p[0] == 0x47));
        let pat_start = 5 + data[4] as usize + 1;
        let pat = &data[pat_start..pat_start + 16];
        assert_eq!(crc32(pat), 0);
        let pmt_start = 188 + 5 + data[188 + 4] as usize + 1;
        assert_eq!(data[pmt_start], 2);
        assert_eq!(data[pmt_start + 12], 0x24);
        assert_eq!(data[376 + 5] & 0x50, 0x50);
        let seq = data
            .chunks_exact(188)
            .skip(2)
            .map(|p| p[3] & 15)
            .collect::<Vec<_>>();
        assert!(seq.windows(2).all(|s| s[1] == (s[0] + 1) & 15));
    }
    #[test]
    fn missing_device_timestamps_use_measured_burst_cadence_instead_of_fixed_twenty_fps() {
        let mut clock = FrameClock::default();
        let mut before = 0;
        let mut last = 0;
        for batch in 0..7 {
            for _ in 0..10 {
                last = clock.timestamp(0, batch * 1000);
            }
            if batch == 2 {
                before = last;
            }
        }
        assert!(
            last - before > 3500 * 90,
            "missing timestamps compressed burst playback"
        );
        assert_eq!(clock.diagnostics.timestamp_samples, 0);
        assert!(clock.diagnostics.timestamp_fallbacks > 0);
    }
}
