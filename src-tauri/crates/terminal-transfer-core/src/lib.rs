use std::time::{Duration, Instant};

const ZPAD: u8 = b'*';
const ZDLE: u8 = 0x18;
const ZBIN: u8 = b'A';
const ZHEX: u8 = b'B';
const ZBIN32: u8 = b'C';
const HEADER_PAYLOAD_SIZE: usize = 5;
const DEFAULT_PENDING_LIMIT: usize = 256 * 1024;

pub const ZMODEM_CANCEL_SEQUENCE: &[u8] =
    b"\x18\x18\x18\x18\x18\x18\x18\x18\x08\x08\x08\x08\x08\x08\x08\x08";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransferDirection {
    Upload,
    Download,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HeaderEncoding {
    Binary16,
    Hex16,
    Binary32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ZmodemDetection {
    pub direction: TransferDirection,
    pub encoding: HeaderEncoding,
    pub frame_type: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StreamMode {
    Normal,
    AwaitingDecision,
    Transferring,
    Recovering,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StreamEvent {
    TerminalData(Vec<u8>),
    Detected(ZmodemDetection),
    ProtocolData(Vec<u8>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StreamError {
    PendingBufferOverflow { limit: usize },
    InvalidTransition,
}

#[derive(Default)]
pub struct ZmodemProbe {
    pending: Vec<u8>,
}

impl ZmodemProbe {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn inspect(&mut self, data: &[u8]) -> ProbeOutput {
        let mut combined = Vec::with_capacity(self.pending.len() + data.len());
        combined.append(&mut self.pending);
        combined.extend_from_slice(data);

        let mut index = 0;
        while index < combined.len() {
            let Some(relative) = combined[index..].iter().position(|byte| *byte == ZPAD) else {
                break;
            };
            let start = index + relative;
            match parse_candidate(&combined[start..]) {
                Candidate::Valid { detection } => {
                    return ProbeOutput {
                        terminal_data: combined[..start].to_vec(),
                        protocol_data: combined[start..].to_vec(),
                        detection: Some(detection),
                    };
                }
                Candidate::Incomplete => {
                    self.pending.extend_from_slice(&combined[start..]);
                    return ProbeOutput {
                        terminal_data: combined[..start].to_vec(),
                        protocol_data: Vec::new(),
                        detection: None,
                    };
                }
                Candidate::Invalid => index = start + 1,
            }
        }

        ProbeOutput {
            terminal_data: combined,
            protocol_data: Vec::new(),
            detection: None,
        }
    }

    pub fn flush(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.pending)
    }

    pub fn reset(&mut self) {
        self.pending.clear();
    }
}

pub struct ProbeOutput {
    pub terminal_data: Vec<u8>,
    pub protocol_data: Vec<u8>,
    pub detection: Option<ZmodemDetection>,
}

pub struct TerminalStreamMux {
    mode: StreamMode,
    probe: ZmodemProbe,
    probe_pending_since: Option<Instant>,
    pending_protocol: Vec<u8>,
    pending_limit: usize,
}

impl Default for TerminalStreamMux {
    fn default() -> Self {
        Self::new(DEFAULT_PENDING_LIMIT)
    }
}

impl TerminalStreamMux {
    pub fn new(pending_limit: usize) -> Self {
        Self {
            mode: StreamMode::Normal,
            probe: ZmodemProbe::new(),
            probe_pending_since: None,
            pending_protocol: Vec::new(),
            pending_limit: pending_limit.max(1),
        }
    }

    pub const fn mode(&self) -> StreamMode {
        self.mode
    }

    pub fn push_remote(&mut self, data: &[u8]) -> Result<Vec<StreamEvent>, StreamError> {
        match self.mode {
            StreamMode::Normal => {
                let had_pending = !self.probe.pending.is_empty();
                let output = self.probe.inspect(data);
                let has_pending = !self.probe.pending.is_empty();
                self.probe_pending_since = match (had_pending, has_pending) {
                    (false, true) => Some(Instant::now()),
                    (true, true) => self.probe_pending_since,
                    (_, false) => None,
                };
                let mut events = Vec::with_capacity(2);
                if !output.terminal_data.is_empty() {
                    events.push(StreamEvent::TerminalData(output.terminal_data));
                }
                if let Some(detection) = output.detection {
                    self.probe_pending_since = None;
                    self.pending_protocol = output.protocol_data;
                    self.ensure_pending_limit()?;
                    self.mode = StreamMode::AwaitingDecision;
                    events.push(StreamEvent::Detected(detection));
                }
                Ok(events)
            }
            StreamMode::AwaitingDecision => {
                self.pending_protocol.extend_from_slice(data);
                self.ensure_pending_limit()?;
                Ok(Vec::new())
            }
            StreamMode::Transferring => Ok(if data.is_empty() {
                Vec::new()
            } else {
                vec![StreamEvent::ProtocolData(data.to_vec())]
            }),
            StreamMode::Recovering => Ok(if data.is_empty() {
                Vec::new()
            } else {
                vec![StreamEvent::ProtocolData(data.to_vec())]
            }),
        }
    }

    pub fn accept(&mut self) -> Result<Vec<u8>, StreamError> {
        if self.mode != StreamMode::AwaitingDecision {
            return Err(StreamError::InvalidTransition);
        }
        self.mode = StreamMode::Transferring;
        Ok(std::mem::take(&mut self.pending_protocol))
    }

    pub fn reject(&mut self) -> Result<(), StreamError> {
        if self.mode != StreamMode::AwaitingDecision {
            return Err(StreamError::InvalidTransition);
        }
        self.pending_protocol.clear();
        self.mode = StreamMode::Recovering;
        Ok(())
    }

    pub fn begin_recovery(&mut self) {
        self.pending_protocol.clear();
        self.probe.reset();
        self.probe_pending_since = None;
        self.mode = StreamMode::Recovering;
    }

    pub fn restore_terminal(&mut self) {
        self.pending_protocol.clear();
        self.probe.reset();
        self.probe_pending_since = None;
        self.mode = StreamMode::Normal;
    }

    pub fn flush_terminal_data(&mut self) -> Vec<u8> {
        if self.mode == StreamMode::Normal {
            self.probe_pending_since = None;
            self.probe.flush()
        } else {
            Vec::new()
        }
    }

    pub fn flush_stale_terminal_data(&mut self, max_age: Duration) -> Vec<u8> {
        if self.mode != StreamMode::Normal
            || self
                .probe_pending_since
                .is_none_or(|since| since.elapsed() < max_age)
        {
            return Vec::new();
        }
        self.flush_terminal_data()
    }

    fn ensure_pending_limit(&mut self) -> Result<(), StreamError> {
        if self.pending_protocol.len() <= self.pending_limit {
            return Ok(());
        }
        self.pending_protocol.clear();
        self.mode = StreamMode::Recovering;
        Err(StreamError::PendingBufferOverflow {
            limit: self.pending_limit,
        })
    }
}

enum Candidate {
    Valid { detection: ZmodemDetection },
    Incomplete,
    Invalid,
}

fn parse_candidate(data: &[u8]) -> Candidate {
    if data.first() != Some(&ZPAD) {
        return Candidate::Invalid;
    }

    let mut cursor = 0;
    while data.get(cursor) == Some(&ZPAD) {
        cursor += 1;
    }
    if cursor == data.len() {
        return Candidate::Incomplete;
    }
    if data.get(cursor) != Some(&ZDLE) {
        return Candidate::Invalid;
    }
    cursor += 1;
    let Some(encoding_byte) = data.get(cursor).copied() else {
        return Candidate::Incomplete;
    };
    cursor += 1;

    let (encoding, decoded_len) = match encoding_byte {
        ZBIN => (HeaderEncoding::Binary16, HEADER_PAYLOAD_SIZE + 2),
        ZHEX => (HeaderEncoding::Hex16, (HEADER_PAYLOAD_SIZE + 2) * 2),
        ZBIN32 => (HeaderEncoding::Binary32, HEADER_PAYLOAD_SIZE + 4),
        _ => return Candidate::Invalid,
    };

    let Some((header, _)) = decode_header_bytes(data, cursor, encoding, decoded_len) else {
        return Candidate::Incomplete;
    };
    if !header.crc_valid {
        return Candidate::Invalid;
    }

    let direction = match header.payload[0] {
        0 | 4 => TransferDirection::Download,
        1 => TransferDirection::Upload,
        _ => return Candidate::Invalid,
    };

    Candidate::Valid {
        detection: ZmodemDetection {
            direction,
            encoding,
            frame_type: header.payload[0],
        },
    }
}

struct DecodedHeader {
    payload: [u8; HEADER_PAYLOAD_SIZE],
    crc_valid: bool,
}

fn decode_header_bytes(
    data: &[u8],
    start: usize,
    encoding: HeaderEncoding,
    expected_len: usize,
) -> Option<(DecodedHeader, usize)> {
    let mut decoded = Vec::with_capacity(expected_len);
    let mut cursor = start;

    if encoding == HeaderEncoding::Hex16 {
        let end = cursor.checked_add(expected_len)?;
        let bytes = data.get(cursor..end)?;
        let mut index = 0;
        while index < bytes.len() {
            decoded.push((hex_value(bytes[index])? << 4) | hex_value(bytes[index + 1])?);
            index += 2;
        }
        cursor = end;
    } else {
        while decoded.len() < expected_len {
            let byte = *data.get(cursor)?;
            cursor += 1;
            if byte == ZDLE {
                let escaped = *data.get(cursor)?;
                cursor += 1;
                decoded.push(unescape_zdle(escaped));
            } else {
                decoded.push(byte);
            }
        }
    }

    let payload: [u8; HEADER_PAYLOAD_SIZE] = decoded[..HEADER_PAYLOAD_SIZE].try_into().ok()?;
    let crc_valid = match encoding {
        HeaderEncoding::Binary32 => {
            decoded[HEADER_PAYLOAD_SIZE..] == crc32_iso_hdlc(&payload).to_le_bytes()
        }
        HeaderEncoding::Binary16 | HeaderEncoding::Hex16 => {
            decoded[HEADER_PAYLOAD_SIZE..] == crc16_xmodem(&payload).to_be_bytes()
        }
    };

    Some((DecodedHeader { payload, crc_valid }, cursor))
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn unescape_zdle(byte: u8) -> u8 {
    match byte {
        0x40..=0x5f | 0xc0..=0xdf => byte ^ 0x40,
        0x6c => 0x7f,
        0x6d => 0xff,
        _ => byte,
    }
}

fn crc16_xmodem(data: &[u8]) -> u16 {
    let mut crc = 0u16;
    for byte in data {
        crc ^= u16::from(*byte) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 {
                (crc << 1) ^ 0x1021
            } else {
                crc << 1
            };
        }
    }
    crc
}

fn crc32_iso_hdlc(data: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for byte in data {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xedb8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;
    use zmodem2::{Action, Receiver, Sender};

    fn hex_header(frame_type: u8) -> Vec<u8> {
        let payload = [frame_type, 0, 0, 0, 0];
        let mut encoded = payload.to_vec();
        encoded.extend_from_slice(&crc16_xmodem(&payload).to_be_bytes());
        let mut result = b"**\x18B".to_vec();
        for byte in encoded {
            result.extend_from_slice(format!("{byte:02x}").as_bytes());
        }
        result.extend_from_slice(b"\r\n\x11");
        result
    }

    fn binary32_header(frame_type: u8) -> Vec<u8> {
        let payload = [frame_type, 0, 0, 0, 0];
        let mut encoded = payload.to_vec();
        encoded.extend_from_slice(&crc32_iso_hdlc(&payload).to_le_bytes());
        let mut result = b"*\x18C".to_vec();
        for byte in encoded {
            if matches!(byte, 0x0d | 0x10 | 0x11 | 0x13 | 0x18 | 0x7f | 0xff) {
                result.push(ZDLE);
                result.push(match byte {
                    0x7f => 0x6c,
                    0xff => 0x6d,
                    _ => byte ^ 0x40,
                });
            } else {
                result.push(byte);
            }
        }
        result
    }

    #[test]
    fn normal_data_is_delayed_only_for_a_possible_header_prefix() {
        let mut probe = ZmodemProbe::new();
        let first = probe.inspect(b"shell output **");
        assert_eq!(first.terminal_data, b"shell output ");
        assert!(first.detection.is_none());

        let second = probe.inspect(b"not-zmodem");
        assert_eq!(second.terminal_data, b"**not-zmodem");
        assert!(second.detection.is_none());
    }

    #[test]
    fn detects_valid_hex_headers_at_every_packet_boundary_without_leaking_bytes() {
        let header = hex_header(1);
        for split in 0..=header.len() {
            let mut probe = ZmodemProbe::new();
            let mut terminal = Vec::new();
            let first = probe.inspect(&[b"prompt> ".as_slice(), &header[..split]].concat());
            terminal.extend(first.terminal_data);
            let detection = if first.detection.is_some() {
                first.detection
            } else {
                let second = probe.inspect(&header[split..]);
                terminal.extend(second.terminal_data);
                second.detection
            };

            assert_eq!(terminal, b"prompt> ", "split={split}");
            assert_eq!(
                detection.map(|value| value.direction),
                Some(TransferDirection::Upload),
                "split={split}"
            );
        }
    }

    #[test]
    fn detects_binary32_download_header() {
        let header = binary32_header(0);
        let mut probe = ZmodemProbe::new();
        let output = probe.inspect(&header);
        assert_eq!(
            output.detection.map(|value| value.direction),
            Some(TransferDirection::Download)
        );
        assert_eq!(output.protocol_data, header);
    }

    #[test]
    fn detects_initial_frames_emitted_by_the_protocol_engine() {
        let mut sender = Sender::new().unwrap();
        let sender_wire = match sender.poll() {
            Action::WriteWire(bytes) => bytes.to_vec(),
            action => panic!("unexpected sender action: {action:?}"),
        };
        assert_eq!(
            ZmodemProbe::new()
                .inspect(&sender_wire)
                .detection
                .map(|value| value.direction),
            Some(TransferDirection::Download)
        );

        let mut receiver = Receiver::new().unwrap();
        let receiver_wire = match receiver.poll() {
            Action::WriteWire(bytes) => bytes.to_vec(),
            action => panic!("unexpected receiver action: {action:?}"),
        };
        assert_eq!(
            ZmodemProbe::new()
                .inspect(&receiver_wire)
                .detection
                .map(|value| value.direction),
            Some(TransferDirection::Upload)
        );
    }

    #[test]
    fn decodes_zdle_delete_and_ff_escapes() {
        assert_eq!(unescape_zdle(0x6c), 0x7f);
        assert_eq!(unescape_zdle(0x6d), 0xff);
    }

    #[test]
    fn invalid_crc_does_not_claim_the_terminal_stream() {
        let mut header = hex_header(1);
        header[10] = if header[10] == b'0' { b'1' } else { b'0' };
        let mut probe = ZmodemProbe::new();
        let output = probe.inspect(&header);
        assert!(output.detection.is_none());
        assert_eq!(output.terminal_data, header);
    }

    #[test]
    fn mux_buffers_protocol_until_accepted() {
        let header = hex_header(1);
        let mut mux = TerminalStreamMux::new(1024);
        let events = mux.push_remote(&header).unwrap();
        assert!(matches!(events.as_slice(), [StreamEvent::Detected(_)]));
        mux.push_remote(b"more").unwrap();
        assert_eq!(mux.accept().unwrap(), [header, b"more".to_vec()].concat());
        assert_eq!(mux.mode(), StreamMode::Transferring);
    }

    #[test]
    fn mux_flushes_an_incomplete_header_prefix_after_timeout() {
        let mut mux = TerminalStreamMux::new(1024);
        let events = mux.push_remote(b"grep *").unwrap();
        assert_eq!(events, vec![StreamEvent::TerminalData(b"grep ".to_vec())]);

        assert!(mux
            .flush_stale_terminal_data(Duration::from_secs(1))
            .is_empty());
        assert_eq!(mux.flush_stale_terminal_data(Duration::ZERO), b"*".to_vec());
        assert!(mux.flush_stale_terminal_data(Duration::ZERO).is_empty());
    }

    #[test]
    fn mux_bounds_the_decision_buffer() {
        let header = hex_header(1);
        let mut mux = TerminalStreamMux::new(header.len() + 1);
        mux.push_remote(&header).unwrap();
        let error = mux.push_remote(b"overflow").unwrap_err();
        assert!(matches!(error, StreamError::PendingBufferOverflow { .. }));
        assert_eq!(mux.mode(), StreamMode::Recovering);
    }

    #[test]
    fn mux_exposes_recovery_bytes_for_prompt_restoration() {
        let mut mux = TerminalStreamMux::new(1024);
        mux.begin_recovery();
        assert_eq!(
            mux.push_remote(b"\r\nprompt> ").unwrap(),
            vec![StreamEvent::ProtocolData(b"\r\nprompt> ".to_vec())]
        );
    }
}
