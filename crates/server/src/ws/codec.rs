use base64::prelude::*;
use ring::digest::{digest, SHA1_FOR_LEGACY_USE_ONLY};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub const WS_GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

/// Compute Sec-WebSocket-Accept header value from Sec-WebSocket-Key
pub fn compute_accept_key(sec_websocket_key: &str) -> String {
    let concat = format!("{}{}", sec_websocket_key.trim(), WS_GUID);
    let hash = digest(&SHA1_FOR_LEGACY_USE_ONLY, concat.as_bytes());
    BASE64_STANDARD.encode(hash.as_ref())
}

/// WebSocket protocol message
#[derive(Debug, Clone, PartialEq)]
pub enum WsMessage {
    Text(String),
    Binary(Vec<u8>),
    Ping(Vec<u8>),
    Pong(Vec<u8>),
    Close,
}

/// WebSocket reader half
pub struct WsReader<R> {
    reader: R,
}

impl<R: AsyncRead + Unpin> WsReader<R> {
    pub fn new(reader: R) -> Self {
        Self { reader }
    }

    /// Read a single complete WebSocket frame
    pub async fn read_message(&mut self) -> std::io::Result<WsMessage> {
        let mut header = [0u8; 2];
        self.reader.read_exact(&mut header).await?;

        let _fin = (header[0] & 0x80) != 0;
        let opcode = header[0] & 0x0F;
        let masked = (header[1] & 0x80) != 0;
        let mut payload_len = (header[1] & 0x7F) as u64;

        if payload_len == 126 {
            let mut ext = [0u8; 2];
            self.reader.read_exact(&mut ext).await?;
            payload_len = u16::from_be_bytes(ext) as u64;
        } else if payload_len == 127 {
            let mut ext = [0u8; 8];
            self.reader.read_exact(&mut ext).await?;
            payload_len = u64::from_be_bytes(ext);
        }

        let mask = if masked {
            let mut mask_bytes = [0u8; 4];
            self.reader.read_exact(&mut mask_bytes).await?;
            Some(mask_bytes)
        } else {
            None
        };

        let mut payload = vec![0u8; payload_len as usize];
        self.reader.read_exact(&mut payload).await?;

        if let Some(mask) = mask {
            for (i, byte) in payload.iter_mut().enumerate() {
                *byte ^= mask[i % 4];
            }
        }

        match opcode {
            0x1 => {
                let text = String::from_utf8(payload)
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
                Ok(WsMessage::Text(text))
            }
            0x2 => Ok(WsMessage::Binary(payload)),
            0x8 => Ok(WsMessage::Close),
            0x9 => Ok(WsMessage::Ping(payload)),
            0xA => Ok(WsMessage::Pong(payload)),
            _ => Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Unsupported opcode: {}", opcode),
            )),
        }
    }
}

/// WebSocket writer half
pub struct WsWriter<W> {
    writer: W,
}

impl<W: AsyncWrite + Unpin> WsWriter<W> {
    pub fn new(writer: W) -> Self {
        Self { writer }
    }

    /// Write a WebSocket frame to the stream
    pub async fn write_message(&mut self, msg: &WsMessage) -> std::io::Result<()> {
        let (opcode, payload) = match msg {
            WsMessage::Text(text) => (0x1, text.as_bytes()),
            WsMessage::Binary(bin) => (0x2, bin.as_slice()),
            WsMessage::Ping(p) => (0x9, p.as_slice()),
            WsMessage::Pong(p) => (0xA, p.as_slice()),
            WsMessage::Close => (0x8, &[][..]),
        };

        let mut header = Vec::with_capacity(10);
        header.push(0x80 | opcode); // FIN + opcode

        let len = payload.len();
        if len < 126 {
            header.push(len as u8);
        } else if len <= 0xFFFF {
            header.push(126);
            header.extend_from_slice(&(len as u16).to_be_bytes());
        } else {
            header.push(127);
            header.extend_from_slice(&(len as u64).to_be_bytes());
        }

        self.writer.write_all(&header).await?;
        if !payload.is_empty() {
            self.writer.write_all(payload).await?;
        }
        self.writer.flush().await?;
        Ok(())
    }
}
