use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TerminalTransferDirection {
    SendToRemote,
    ReceiveFromRemote,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalTransferRequest {
    pub protocol: String,
    pub direction: TerminalTransferDirection,
    pub message: String,
}

#[derive(Clone, Debug)]
pub enum TerminalTransferProbe {
    TerminalData(Vec<u8>),
    Detected(TerminalTransferRequest, Vec<u8>),
}

#[derive(Default)]
pub struct ZmodemDetector {
    tail: Vec<u8>,
}

impl ZmodemDetector {
    pub fn new() -> Self {
        Self { tail: Vec::new() }
    }

    pub fn inspect(&mut self, data: &[u8]) -> TerminalTransferProbe {
        let mut combined = Vec::with_capacity(self.tail.len() + data.len());
        combined.extend_from_slice(&self.tail);
        combined.extend_from_slice(data);

        let detected = find_zmodem_signature(&combined);
        self.tail = combined
            .iter()
            .rev()
            .take(MAX_SIGNATURE_LEN - 1)
            .copied()
            .collect::<Vec<_>>();
        self.tail.reverse();

        if let Some(direction) = detected {
            self.tail.clear();
            TerminalTransferProbe::Detected(
                TerminalTransferRequest {
                    protocol: "zmodem".to_string(),
                    direction,
                    message: "ZMODEM 文件传输暂不支持，请使用 SFTP 文件面板上传或下载文件。"
                        .to_string(),
                },
                data.to_vec(),
            )
        } else {
            TerminalTransferProbe::TerminalData(data.to_vec())
        }
    }
}

const MAX_SIGNATURE_LEN: usize = 7;
const ZMODEM_SIGNATURES: [&[u8]; 3] = [b"**\x18B", b"*\x18A", b"*\x18C"];

fn find_zmodem_signature(data: &[u8]) -> Option<TerminalTransferDirection> {
    for signature in ZMODEM_SIGNATURES {
        if let Some(index) = data
            .windows(signature.len())
            .position(|window| window == signature)
        {
            return Some(infer_direction(data, index + signature.len()));
        }
    }
    None
}

fn infer_direction(data: &[u8], frame_type_start: usize) -> TerminalTransferDirection {
    let frame_type = data.get(frame_type_start..frame_type_start + 2);
    match frame_type {
        Some(b"01") => TerminalTransferDirection::SendToRemote,
        Some(b"00") | Some(b"04") => TerminalTransferDirection::ReceiveFromRemote,
        _ => TerminalTransferDirection::ReceiveFromRemote,
    }
}

#[cfg(test)]
mod tests {
    use super::{TerminalTransferDirection, TerminalTransferProbe, ZmodemDetector};

    #[test]
    fn passes_normal_terminal_bytes_through() {
        let mut detector = ZmodemDetector::new();
        match detector.inspect(b"hello shell\r\n") {
            TerminalTransferProbe::TerminalData(data) => assert_eq!(data, b"hello shell\r\n"),
            TerminalTransferProbe::Detected(_, _) => panic!("ordinary terminal output was detected"),
        }
    }

    #[test]
    fn detects_complete_zmodem_signature() {
        let mut detector = ZmodemDetector::new();
        match detector.inspect(b"**\x18B00000000000000") {
            TerminalTransferProbe::Detected(request, data) => {
                assert_eq!(request.protocol, "zmodem");
                assert!(matches!(
                    request.direction,
                    TerminalTransferDirection::ReceiveFromRemote
                ));
                assert_eq!(data, b"**\x18B00000000000000");
            }
            TerminalTransferProbe::TerminalData(_) => panic!("zmodem signature was not detected"),
        }
    }

    #[test]
    fn detects_split_zmodem_signature() {
        let mut detector = ZmodemDetector::new();
        assert!(matches!(
            detector.inspect(b"prefix **"),
            TerminalTransferProbe::TerminalData(_)
        ));

        match detector.inspect(b"\x18B01000000000000") {
            TerminalTransferProbe::Detected(request, _) => {
                assert!(matches!(
                    request.direction,
                    TerminalTransferDirection::SendToRemote
                ));
            }
            TerminalTransferProbe::TerminalData(_) => {
                panic!("split zmodem signature was not detected")
            }
        }
    }
}
