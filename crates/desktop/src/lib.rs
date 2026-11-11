//! FaultDeck desktop UI — Tauri shell.
//!
//! The Rust side is the only component that talks to the coordinator: it
//! speaks the existing newline-framed JSON wire protocol
//! (`WireMessage` / `AgentCommand` / `AgentResponse` from
//! `faultdeck-coordinator`) so the UI adds a view, not a second API.
//!
//! Design: docs/design/desktop-ui.md.

use serde::Serialize;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::time::Duration;

mod wire {
    use serde::{Deserialize, Serialize};

    /// Control message a controller session sends to the coordinator.
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct WireMessage {
        pub token: Option<String>,
        pub command: AgentCommand,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(tag = "cmd", rename_all = "snake_case")]
    pub enum AgentCommand {
        Register(RegisterRequest),
        Heartbeat {
            id: String,
        },
        Inject {
            id: String,
            injection: faultdeck_core::Injection,
        },
        Rollback {
            id: String,
        },
        FetchReport {
            id: String,
            scenario_hash: String,
        },
        ListAgents,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct RegisterRequest {
        pub id: String,
        #[serde(default)]
        pub capabilities: Vec<String>,
        #[serde(default)]
        pub token: Option<String>,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct AgentIdentity {
        pub id: String,
        #[serde(default)]
        pub capabilities: Vec<String>,
        #[serde(default)]
        pub last_seen_ms: u64,
        #[serde(default)]
        pub online: bool,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(tag = "status", rename_all = "snake_case")]
    pub enum AgentResponse {
        Ok {
            message: String,
        },
        Error {
            message: String,
        },
        Registered {
            id: String,
        },
        Queued {
            agent_id: String,
            pending: u64,
        },
        TimedOut {
            agent_id: String,
        },
        Agents {
            agents: Vec<AgentIdentity>,
            available: Vec<String>,
        },
        Report {
            report: faultdeck_report::RunReport,
        },
    }
}

use wire::*;

/// Errors surfaced to the frontend as a plain string.
#[derive(Debug, Serialize)]
pub struct UiError {
    pub message: String,
}

impl From<String> for UiError {
    fn from(message: String) -> Self {
        Self { message }
    }
}

impl From<std::io::Error> for UiError {
    fn from(e: std::io::Error) -> Self {
        Self {
            message: e.to_string(),
        }
    }
}

impl From<serde_json::Error> for UiError {
    fn from(e: serde_json::Error) -> Self {
        Self {
            message: format!("wire decode failed: {e}"),
        }
    }
}

/// Send one command to the coordinator and return the decoded response.
///
/// One connection per command keeps the UI stateless: the coordinator's own
/// state (agents, queues, reports) is the source of truth.
fn send_command(
    host: &str,
    port: u16,
    token: Option<String>,
    command: AgentCommand,
) -> Result<AgentResponse, UiError> {
    let addr = format!("{host}:{port}");
    let mut stream = TcpStream::connect(&addr)
        .map_err(|e| UiError::from(format!("coordinator unreachable at {addr}: {e}")))?;
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    stream.set_write_timeout(Some(Duration::from_secs(10)))?;

    let message = WireMessage { token, command };
    let mut line = serde_json::to_string(&message)?;
    line.push('\n');
    stream.write_all(line.as_bytes())?;
    stream.flush()?;

    let mut reader = BufReader::new(stream);
    let mut response = String::new();
    reader.read_line(&mut response)?;
    if response.trim().is_empty() {
        return Err(UiError::from(
            "coordinator closed the connection without replying".to_string(),
        ));
    }
    Ok(serde_json::from_str(response.trim())?)
}

/// Fleet view: every registered agent with live online status.
#[tauri::command]
fn coordinator_list(
    host: String,
    port: u16,
    token: Option<String>,
) -> Result<AgentResponse, UiError> {
    send_command(&host, port, token, AgentCommand::ListAgents)
}

/// Inject a fault into one agent.
#[tauri::command]
fn coordinator_inject(
    host: String,
    port: u16,
    token: Option<String>,
    agent_id: String,
    injection: faultdeck_core::Injection,
) -> Result<AgentResponse, UiError> {
    send_command(
        &host,
        port,
        token,
        AgentCommand::Inject {
            id: agent_id,
            injection,
        },
    )
}

/// Roll back the fault currently applied to one agent.
#[tauri::command]
fn coordinator_rollback(
    host: String,
    port: u16,
    token: Option<String>,
    agent_id: String,
) -> Result<AgentResponse, UiError> {
    send_command(&host, port, token, AgentCommand::Rollback { id: agent_id })
}

/// Fetch the archived report for a (agent, scenario hash) pair.
#[tauri::command]
fn coordinator_report(
    host: String,
    port: u16,
    token: Option<String>,
    agent_id: String,
    scenario_hash: String,
) -> Result<AgentResponse, UiError> {
    send_command(
        &host,
        port,
        token,
        AgentCommand::FetchReport {
            id: agent_id,
            scenario_hash,
        },
    )
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            coordinator_list,
            coordinator_inject,
            coordinator_rollback,
            coordinator_report
        ])
        .run(tauri::generate_context!())
        .expect("error while running faultdeck desktop");
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The wire shapes are a contract with the coordinator: assert the exact
    /// JSON the UI emits, so a rename on either side fails here first.
    #[test]
    fn list_agents_wire_shape() {
        let msg = WireMessage {
            token: Some("t".into()),
            command: AgentCommand::ListAgents,
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert_eq!(json, r#"{"token":"t","command":{"cmd":"list_agents"}}"#);
    }

    #[test]
    fn inject_wire_shape() {
        let injection: faultdeck_core::Injection = serde_json::from_str(
            r#"{"fault":{"kind":"latency","milliseconds":100},"target":{"source":null,"destination":"10.0.0.0/24","destination_port":9000}}"#,
        )
        .unwrap();
        let msg = WireMessage {
            token: None,
            command: AgentCommand::Inject {
                id: "a1".into(),
                injection,
            },
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains(r#""cmd":"inject""#));
        assert!(json.contains(r#""id":"a1""#));
        assert!(json.contains(r#""kind":"latency""#));
    }

    #[test]
    fn rollback_wire_shape() {
        let msg = WireMessage {
            token: None,
            command: AgentCommand::Rollback { id: "a1".into() },
        };
        assert_eq!(
            serde_json::to_string(&msg).unwrap(),
            r#"{"token":null,"command":{"cmd":"rollback","id":"a1"}}"#
        );
    }

    #[test]
    fn parses_agents_response() {
        let raw = r#"{"status":"agents","agents":[{"id":"a1","capabilities":["netem"],"last_seen_ms":123,"online":true}],"available":["a1"]}"#;
        match serde_json::from_str::<AgentResponse>(raw).unwrap() {
            AgentResponse::Agents { agents, available } => {
                assert_eq!(agents.len(), 1);
                assert!(agents[0].online);
                assert_eq!(available, vec!["a1".to_string()]);
            }
            other => panic!("wrong variant: {other:?}"),
        }
    }

    #[test]
    fn parses_error_response() {
        let raw = r#"{"status":"error","message":"agent offline"}"#;
        match serde_json::from_str::<AgentResponse>(raw).unwrap() {
            AgentResponse::Error { message } => assert_eq!(message, "agent offline"),
            other => panic!("wrong variant: {other:?}"),
        }
    }
}
