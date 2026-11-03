use std::collections::HashSet;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};

use faultdeck_core::{overlaps_management, Injection};
use faultdeck_netem::NetemAdapter;
use firewall::{is_disconnect, Iptables};
use ipnet::IpNet;
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub mod firewall;
pub mod transport;
pub const MAX_INJECTION_BYTES: usize = 64 * 1024;

#[derive(Debug, Error)]
pub enum AgentError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid command {reason}")]
    InvalidCommand { reason: String },
    #[error("unauthorized: missing or invalid token")]
    Unauthorized,
    #[error("adapter error: {0}")]
    Adapter(#[from] faultdeck_netem::AdapterError),
}

pub type Result<T> = std::result::Result<T, AgentError>;

/// Constant-time byte equality to avoid timing side channels when comparing
/// an auth token. The length difference is folded into the accumulator and
/// the loop runs over the longer slice (missing bytes read as zero), so an
/// unequal length does not return early and leak the expected length.
pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    let n = a.len().max(b.len());
    let len_diff = (a.len() as u64) ^ (b.len() as u64);
    // Fold every byte of the length difference so a mismatch is reflected in
    // the result even before the byte loop finishes.
    let mut diff = (len_diff | (len_diff >> 8) | (len_diff >> 16) | (len_diff >> 32)) as u8;
    for i in 0..n {
        let x = a.get(i).copied().unwrap_or(0);
        let y = b.get(i).copied().unwrap_or(0);
        diff |= x ^ y;
    }
    diff == 0
}

/// Read a newline-delimited line with a hard byte ceiling.
/// `read_line` would otherwise buffer an unbounded attacker-controlled
/// payload before the size check could run.
pub fn read_line_limited<R: BufRead>(reader: &mut R, max_bytes: usize) -> std::io::Result<String> {
    let mut buffer = Vec::new();
    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            break;
        }
        let newline = available.iter().position(|&byte| byte == b'\n');
        let take = newline.map(|index| index + 1).unwrap_or(available.len());
        if buffer.len() + take > max_bytes {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "line exceeds maximum size",
            ));
        }
        buffer.extend_from_slice(&available[..take]);
        reader.consume(take);
        if newline.is_some() {
            break;
        }
    }
    String::from_utf8(buffer).map_err(|_| {
        std::io::Error::new(std::io::ErrorKind::InvalidData, "line is not valid UTF-8")
    })
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentConfig {
    pub bind: String,
    pub token: Option<String>,
    pub interface: String,
    pub management_networks: Vec<IpNet>,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            bind: "127.0.0.1:0".into(),
            token: None,
            interface: "eth0".into(),
            management_networks: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum Request {
    Ping,
    Inject(Injection),
    Rollback,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Response {
    Ok { message: String },
    Error { message: String },
}

fn token_ok(config: &AgentConfig, request_token: Option<&str>) -> bool {
    match &config.token {
        None => true,
        Some(expected) => request_token
            .map(|token| constant_time_eq(token.as_bytes(), expected.as_bytes()))
            .unwrap_or(false),
    }
}

/// True if `bind` is a loopback-only address (127.0.0.1 / ::1).
pub fn is_loopback_bind(bind: &str) -> bool {
    let host = bind.rsplit_once(':').map(|(h, _)| h).unwrap_or(bind);
    let host = host.trim_start_matches('[').trim_end_matches(']');
    host == "127.0.0.1" || host == "::1" || host == "localhost"
}

/// Warn and return true when binding a non-loopback address without a token.
pub fn warn_insecure_bind(bind: &str, token: Option<&str>) -> bool {
    let insecure = !is_loopback_bind(bind) && token.is_none();
    if insecure {
        eprintln!(
            "warning: binding {bind} without a token; any reachable host can control the agent"
        );
    }
    insecure
}

pub fn authorize(config: &AgentConfig, token: Option<&str>) -> Result<()> {
    if token_ok(config, token) {
        Ok(())
    } else {
        Err(AgentError::Unauthorized)
    }
}

/// Guard: reject injections whose target overlaps a management network.
pub fn guard_management(config: &AgentConfig, injection: &Injection) -> Result<()> {
    for network in injection.target.networks() {
        if overlaps_management(network, &config.management_networks) {
            return Err(AgentError::InvalidCommand {
                reason: format!("target {network} overlaps management network"),
            });
        }
    }
    Ok(())
}

pub fn process_request<E>(
    config: &AgentConfig,
    adapter: &NetemAdapter<E>,
    firewall: &Iptables,
    request: Request,
    applied: &mut HashSet<String>,
) -> Response
where
    E: faultdeck_netem::CommandExecutor,
{
    match &request {
        Request::Ping => Response::Ok {
            message: "pong".into(),
        },
        Request::Inject(injection) => {
            if let Err(e) = guard_management(config, injection) {
                return Response::Error {
                    message: e.to_string(),
                };
            }
            if !applied.is_empty() {
                return Response::Error {
                    message: "another fault is already applied; rollback first".into(),
                };
            }
            if is_disconnect(injection) {
                match firewall.apply(injection) {
                    Ok(_) => {
                        applied.insert("firewall".into());
                        Response::Ok {
                            message: "fault applied".into(),
                        }
                    }
                    Err(e) => Response::Error {
                        message: e.to_string(),
                    },
                }
            } else {
                match adapter.inject(injection) {
                    Ok(_) => {
                        applied.insert("netem".into());
                        Response::Ok {
                            message: "fault applied".into(),
                        }
                    }
                    Err(e) => Response::Error {
                        message: e.to_string(),
                    },
                }
            }
        }
        Request::Rollback => {
            if applied.is_empty() {
                return Response::Ok {
                    message: "nothing to roll back".into(),
                };
            }
            let kind = applied.iter().next().cloned().unwrap_or_default();
            let result = if kind == "firewall" {
                firewall.rollback().map_err(|e| e.to_string())
            } else {
                adapter.rollback().map(|_| ()).map_err(|e| e.to_string())
            };
            match result {
                Ok(()) => {
                    applied.clear();
                    Response::Ok {
                        message: "fault rolled back".into(),
                    }
                }
                Err(e) => Response::Error { message: e },
            }
        }
    }
}

pub fn handle_connection<E>(
    config: &Arc<AgentConfig>,
    adapter: &NetemAdapter<E>,
    firewall: &Iptables,
    applied: &mut HashSet<String>,
    stream: TcpStream,
) -> Result<()>
where
    E: faultdeck_netem::CommandExecutor,
{
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut writer = stream;
    let first_line = match read_line_limited(&mut reader, MAX_INJECTION_BYTES) {
        Ok(line) => line,
        Err(_) => {
            // Oversized or invalid line: write an explicit error instead of
            // dropping the connection silently (client saw EOF with no message).
            let _ = writer
                .write_all(b"{\"status\":\"error\",\"message\":\"line too large or invalid\"}\n");
            return Ok(());
        }
    };
    let header = first_line.trim();
    if header.is_empty() {
        let _ = writer.write_all(b"{\"status\":\"error\",\"message\":\"missing token header\"}\n");
        return Ok(());
    }
    // Only split the first token off as auth when a token is configured.
    // Without a token, a payload may legitimately contain spaces (e.g.
    // pretty-printed JSON), so the whole header is treated as the payload.
    let (maybe_token, payload) = match (config.token.is_some(), header.split_once(' ')) {
        (true, Some((token, rest))) => (Some(token), rest),
        _ => (None, header),
    };
    if let Err(e) = authorize(config, maybe_token) {
        let _ = writer.write_all(
            format!(
                "{{\"status\":\"error\",\"message\":{}}}\n",
                serde_json::to_string(&e.to_string())?
            )
            .as_bytes(),
        );
        return Ok(());
    }
    if payload.len() > MAX_INJECTION_BYTES {
        let _ = writer
            .write_all(b"{\"status\":\"error\",\"message\":\"injection payload too large\"}\n");
        return Ok(());
    }
    let request: Request = match serde_json::from_str(payload) {
        Ok(request) => request,
        Err(e) => {
            let _ = writer.write_all(
                format!(
                    "{{\"status\":\"error\",\"message\":{}}}\n",
                    serde_json::to_string(&format!("invalid json: {e}"))?
                )
                .as_bytes(),
            );
            return Ok(());
        }
    };
    if let Request::Inject(ref injection) = request {
        if let Err(e) = faultdeck_core::validate_injection(injection) {
            let _ = writer.write_all(
                format!(
                    "{{\"status\":\"error\",\"message\":{}}}\n",
                    serde_json::to_string(&e.to_string())?
                )
                .as_bytes(),
            );
            return Ok(());
        }
    }
    let response = process_request(config, adapter, firewall, request, applied);
    let _ = writer.write_all(format!("{}\n", serde_json::to_string(&response)?).as_bytes());
    Ok(())
}

pub fn serve(config: AgentConfig, adapter: NetemAdapter, firewall: Iptables) -> Result<()> {
    let listener = TcpListener::bind(&config.bind)?;
    serve_on(config, adapter, firewall, listener, HashSet::new())
}

pub fn serve_on(
    config: AgentConfig,
    adapter: NetemAdapter,
    firewall: Iptables,
    listener: TcpListener,
    applied: HashSet<String>,
) -> Result<()> {
    let config = Arc::new(config);
    eprintln!("agent listening on {}", listener.local_addr()?);
    let applied = Arc::new(Mutex::new(applied));
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let cfg = config.clone();
                let adapter = adapter.clone();
                let mut applied = applied.lock().unwrap();
                let _ = handle_connection(&cfg, &adapter, &firewall, &mut applied, stream);
            }
            Err(e) => eprintln!("accept error: {e}"),
        }
    }
    Ok(())
}
