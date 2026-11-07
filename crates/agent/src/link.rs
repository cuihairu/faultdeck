//! Mode B link client: the agent dials the coordinator (only outbound, so it
//! works behind NAT / double-NAT) and holds one long-lived connection over
//! which the coordinator dispatches inject/rollback commands and the agent
//! returns their execution results. Reconnects with backoff on drop.
//!
//! Wire shape on the link (newline-framed JSON, one message per line):
//! - agent -> coordinator first: `RegisterEnvelope { token, id, capabilities }`
//! - coordinator ack: `{"status":"registered","id":…}` (an `AgentResponse`)
//! - coordinator -> agent: `LinkMessage::Dispatch { request_id, request }`
//! - agent -> coordinator: `LinkMessage::Result { request_id, response }`
//! - agent -> coordinator: `LinkMessage::Heartbeat { id }` (every heartbeat_ms)
//!
//! The same `Request`/`Response` types the direct agent protocol uses are
//! reused, so the protocol stays symmetric across the direct and hub paths.

use std::collections::HashSet;
use std::io::BufReader;
use std::net::{Shutdown, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use faultdeck_netem::{CommandExecutor, NetemAdapter};

use crate::firewall::Iptables;
use crate::transport::{read_frame, send_frame};
use crate::{process_request, AgentConfig, Request, Response};
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const DEFAULT_HEARTBEAT_MS: u64 = 10_000;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LinkMessage {
    /// coordinator -> agent: execute `request` locally and reply `Result`.
    Dispatch { request_id: u64, request: Request },
    /// agent -> coordinator: outcome of a dispatched request.
    Result { request_id: u64, response: Response },
    /// agent -> coordinator: liveness heartbeat over the link.
    Heartbeat { id: String },
}

/// First frame the agent sends to register the link.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RegisterEnvelope {
    pub token: Option<String>,
    pub id: String,
    pub capabilities: Vec<String>,
}

pub struct LinkConfig {
    /// Coordinator address in `host:port` form.
    pub coordinator: String,
    pub agent_id: String,
    pub capabilities: Vec<String>,
    pub heartbeat_ms: u64,
    /// Pause between reconnect attempts after the link drops.
    pub reconnect_backoff: Duration,
}

#[derive(Debug, Error)]
pub enum LinkError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("transport error: {0}")]
    Transport(#[from] crate::transport::TransportError),
    #[error("registration rejected by coordinator")]
    RegisterRejected,
}

pub type Result<T> = std::result::Result<T, LinkError>;

/// One connection lifetime: dial (caller provides the connected stream),
/// register, then serve dispatched requests until the socket drops. The
/// caller loops via [`run_link`] to reconnect.
pub fn link_session<E>(
    cfg: &LinkConfig,
    agent_cfg: &AgentConfig,
    adapter: &NetemAdapter<E>,
    firewall: &Iptables,
    applied: &Mutex<HashSet<String>>,
    stream: TcpStream,
) -> Result<()>
where
    E: CommandExecutor,
{
    let mut reader = BufReader::new(stream.try_clone()?);
    let writer = Arc::new(Mutex::new(stream));

    // Register the link; the coordinator authorizes against the agent token.
    let envelope = RegisterEnvelope {
        token: agent_cfg.token.clone(),
        id: cfg.agent_id.clone(),
        capabilities: cfg.capabilities.clone(),
    };
    send_frame(&mut *writer.lock().unwrap(), &envelope)?;
    let ack: serde_json::Value = read_frame(&mut reader)?;
    let accepted = ack
        .get("status")
        .and_then(|v| v.as_str())
        .map(|s| s == "registered")
        .unwrap_or(false);
    if !accepted {
        return Err(LinkError::RegisterRejected);
    }

    // Heartbeat thread: fire-and-forget over the shared writer.
    let stop = Arc::new(AtomicBool::new(false));
    let heartbeat_writer = writer.clone();
    let heartbeat_stop = stop.clone();
    let heartbeat_id = cfg.agent_id.clone();
    let heartbeat_ms = cfg.heartbeat_ms.max(1);
    std::thread::spawn(move || loop {
        if heartbeat_stop.load(Ordering::Relaxed) {
            break;
        }
        std::thread::sleep(Duration::from_millis(heartbeat_ms));
        if heartbeat_stop.load(Ordering::Relaxed) {
            break;
        }
        let message = LinkMessage::Heartbeat {
            id: heartbeat_id.clone(),
        };
        if send_frame(&mut *heartbeat_writer.lock().unwrap(), &message).is_err() {
            break;
        }
    });

    let result = pump_dispatches(agent_cfg, adapter, firewall, applied, &mut reader, &writer);

    stop.store(true, Ordering::Relaxed);
    // Unblock the reader and the heartbeat writer so both threads exit.
    let _ = writer.lock().unwrap().shutdown(Shutdown::Both);
    result
}

/// Serve loop: execute dispatched requests locally, reply with the result.
/// Runs until the socket drops.
fn pump_dispatches<E>(
    agent_cfg: &AgentConfig,
    adapter: &NetemAdapter<E>,
    firewall: &Iptables,
    applied: &Mutex<HashSet<String>>,
    reader: &mut BufReader<TcpStream>,
    writer: &Arc<Mutex<TcpStream>>,
) -> Result<()>
where
    E: CommandExecutor,
{
    loop {
        match read_frame::<_, LinkMessage>(reader) {
            Ok(LinkMessage::Dispatch {
                request_id,
                request,
            }) => {
                let response = process_request(
                    agent_cfg,
                    adapter,
                    firewall,
                    request,
                    &mut applied.lock().unwrap(),
                );
                let reply = LinkMessage::Result {
                    request_id,
                    response,
                };
                if send_frame(&mut *writer.lock().unwrap(), &reply).is_err() {
                    break;
                }
            }
            // Unsolicited frames (e.g. a control response) are ignored; the
            // agent is a command consumer on this link, not a sender.
            Ok(_) => {}
            Err(_) => break,
        }
    }
    Ok(())
}

/// Mode B reconnect loop: dial the coordinator, run one session, pause, retry.
/// Runs forever; the coordinator drains any commands that were queued while
/// the agent was offline on the next registration.
pub fn run_link<E>(
    cfg: &LinkConfig,
    agent_cfg: &AgentConfig,
    adapter: &NetemAdapter<E>,
    firewall: &Iptables,
    applied: &Mutex<HashSet<String>>,
) -> Result<()>
where
    E: CommandExecutor,
{
    let mut attempt = 0u64;
    loop {
        match TcpStream::connect(&cfg.coordinator) {
            Ok(stream) => {
                attempt = 0;
                eprintln!("faultdeck-agent: linked to coordinator {}", cfg.coordinator);
                if link_session(cfg, agent_cfg, adapter, firewall, applied, stream).is_err() {
                    eprintln!("faultdeck-agent: link dropped; reconnecting");
                }
                std::thread::sleep(cfg.reconnect_backoff);
            }
            Err(e) => {
                attempt += 1;
                eprintln!(
                    "faultdeck-agent: link connect to {} failed ({attempt}): {e}",
                    cfg.coordinator
                );
                std::thread::sleep(cfg.reconnect_backoff);
            }
        }
    }
}
