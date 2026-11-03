use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::time::Duration;

use serde::de::DeserializeOwned;
use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum TransportError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json encode: {0}")]
    Encode(serde_json::Error),
    #[error("json decode: {0}")]
    Decode(serde_json::Error),
    #[error("remote reported error: {0}")]
    Remote(String),
    #[error("stream closed unexpectedly")]
    Closed,
}

pub type Result<T> = std::result::Result<T, TransportError>;

/// One framed JSON line in each direction over a single TCP stream.
pub fn send_frame<W: Write, T: Serialize>(writer: &mut W, value: &T) -> Result<()> {
    let bytes = serde_json::to_vec(value).map_err(TransportError::Encode)?;
    writer.write_all(&bytes)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}

pub fn read_frame<R: BufRead, T: DeserializeOwned>(reader: &mut R) -> Result<T> {
    let mut line = String::new();
    let n = reader.read_line(&mut line)?;
    if n == 0 {
        return Err(TransportError::Closed);
    }
    let value = serde_json::from_str(line.trim()).map_err(TransportError::Decode)?;
    Ok(value)
}

pub fn connect(host: &str, port: u16, retries: u32, backoff: Duration) -> Result<TcpStream> {
    let mut attempt = 0u32;
    loop {
        match TcpStream::connect((host, port)) {
            Ok(stream) => return Ok(stream),
            Err(e) => {
                attempt += 1;
                if attempt >= retries {
                    return Err(TransportError::Io(e));
                }
                std::thread::sleep(backoff);
            }
        }
    }
}

pub fn bind(addr: &str) -> Result<TcpListener> {
    Ok(TcpListener::bind(addr)?)
}

/// Mode A: the agent listens; controller dials in. Single TCP stream carries
/// commands/events/logs/reports bidirectionally using framed JSON lines.
pub struct ModeA {
    listener: TcpListener,
}

impl ModeA {
    pub fn new(addr: &str) -> Result<Self> {
        Ok(Self {
            listener: bind(addr)?,
        })
    }

    pub fn accept(&self) -> Result<(TcpStream, TcpStream)> {
        let (stream, _) = self.listener.accept()?;
        let reader = stream.try_clone()?;
        Ok((reader, stream))
    }

    pub fn addr(&self) -> std::io::Result<std::net::SocketAddr> {
        self.listener.local_addr()
    }
}

/// Mode B: the controller listens; the agent dials out (reverse tunnel through
/// NAT). Long-lived with auto-reconnect.
pub struct ModeB {
    peer_host: String,
    peer_port: u16,
    retries: u32,
    backoff: Duration,
}

impl ModeB {
    pub fn new(
        peer_host: impl Into<String>,
        peer_port: u16,
        retries: u32,
        backoff: Duration,
    ) -> Self {
        Self {
            peer_host: peer_host.into(),
            peer_port,
            retries,
            backoff,
        }
    }

    pub fn connect(&self) -> Result<TcpStream> {
        connect(&self.peer_host, self.peer_port, self.retries, self.backoff)
    }
}

/// Read the remote's framed response, decoding into the caller's response type.
pub fn recv_response<R: BufRead, T: DeserializeOwned>(reader: &mut R) -> Result<T> {
    read_frame(reader)
}

/// Write a request to a connected stream.
pub fn send_request<W: Write, T: Serialize>(writer: &mut W, request: &T) -> Result<()> {
    send_frame(writer, request)
}

pub fn make_reader(stream: &TcpStream) -> Result<BufReader<TcpStream>> {
    Ok(BufReader::new(stream.try_clone()?))
}
