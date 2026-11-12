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
pub mod link;
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
    #[error(
        "refusing to bind {bind} without a token; a daemon that can install \
         DROP rules must fail closed — pass --token or bind a loopback address"
    )]
    InsecureBind { bind: String },
    #[error("adapter error: {0}")]
    Adapter(#[from] faultdeck_netem::AdapterError),
}

pub type Result<T> = std::result::Result<T, AgentError>;

/// Constant-time byte equality to avoid timing side channels when comparing
/// an auth token.
///
/// `b` is the reference/expected value whose length is fixed by the server;
/// `a` is the attacker-controlled presented value (both call sites pass the
/// presented token first and the expected token second). The loop runs
/// exactly `b.len()` iterations, so execution time is independent of the
/// presented length; a `max(a.len(), b.len())` bound would let an attacker
/// infer the expected length from the timing plateau. Missing bytes in `a`
/// read as zero and every byte of the length difference is folded into the
/// accumulator, so any length mismatch still compares unequal.
pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    let len_diff = (a.len() as u64) ^ (b.len() as u64);
    // Fold all 8 bytes of the length difference into the accumulator. The
    // previous `(len_diff | >>8 | >>16 | >>32) as u8` collapsed to zero when
    // the only set bits were in bytes 3 or 5-7 (e.g. a 16MiB gap), letting an
    // all-zero long slice compare equal to an empty one.
    let mut diff = 0u8;
    for byte in len_diff.to_le_bytes() {
        diff |= byte;
    }
    for (i, &y) in b.iter().enumerate() {
        let x = a.get(i).copied().unwrap_or(0);
        diff |= x ^ y;
    }
    diff == 0
}

/// Percent-encode a token for the space-delimited control header so a token
/// that itself contains a space (or another non-unreserved byte) cannot be
/// mistaken for the header delimiter between token and JSON (audit 2026-11-11
/// P2: "token with spaces can't auth"). Unreserved bytes pass through, so a
/// plain token is unchanged on the wire.
pub fn encode_token(token: &str) -> String {
    let mut out = String::with_capacity(token.len());
    for b in token.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push('%');
            out.push_str(&format!("{b:02X}"));
        }
    }
    out
}

/// Inverse of [`encode_token`]. A malformed escape is left verbatim: it will
/// simply not equal the configured token, so authorization fails closed.
pub fn decode_token(encoded: &str) -> String {
    let bytes = encoded.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(hex) = std::str::from_utf8(&bytes[i + 1..i + 3]) {
                if let Ok(v) = u8::from_str_radix(hex, 16) {
                    out.push(v);
                    i += 3;
                    continue;
                }
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
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

/// Fail-closed guard for the listening (Mode A) path: a daemon that can
/// install DROP rules must not serve a non-loopback address without a shared
/// token, because anyone reachable on the network could fault or roll back
/// real traffic. `serve_on` checks the actually-bound listener address with
/// this (so `serve` and any direct caller are covered); link mode (reverse
/// dial) never reaches it because it does not listen.
pub fn require_secure_bind(bind: &str, token: Option<&str>) -> Result<()> {
    if !is_loopback_bind(bind) && token.is_none() {
        return Err(AgentError::InsecureBind {
            bind: bind.to_string(),
        });
    }
    Ok(())
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
    applied: &Mutex<HashSet<String>>,
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
    // The token is percent-encoded on the wire so one that itself contains a
    // space cannot be mistaken for the header delimiter (audit 2026-11-11 P2).
    let (maybe_token, payload) = match (config.token.is_some(), header.split_once(' ')) {
        (true, Some((token_encoded, rest))) => (Some(decode_token(token_encoded)), rest),
        _ => (None, header),
    };
    if let Err(e) = authorize(config, maybe_token.as_deref()) {
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
    // Lock only for the fault-apply itself: auth, parsing, and validation run
    // without the guard so a silent client cannot hold it while reading.
    let response = process_request(
        config,
        adapter,
        firewall,
        request,
        &mut applied.lock().unwrap(),
    );
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
    // Fail closed before accepting anything (audit 2026-11-11 P1.5): an
    // agent that can install DROP rules must not listen on a non-loopback
    // address without a shared token. The bound address is checked, not
    // config.bind, so a mismatched listener cannot slip past.
    let bound = listener.local_addr()?;
    require_secure_bind(&bound.to_string(), config.token.as_deref())?;
    let config = Arc::new(config);
    eprintln!("agent listening on {bound}");
    let applied = Arc::new(Mutex::new(applied));
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                // One thread per connection: a slow or silent client must not
                // stall the accept loop. `applied` is only locked for the
                // duration of `process_request` (inside `handle_connection`),
                // so a client mid-read cannot cut off the rollback channel.
                let cfg = config.clone();
                let adapter = adapter.clone();
                let firewall = firewall.clone();
                let applied = applied.clone();
                std::thread::spawn(move || {
                    let _ = handle_connection(&cfg, &adapter, &firewall, &applied, stream);
                });
            }
            Err(e) => eprintln!("accept error: {e}"),
        }
    }
    Ok(())
}
