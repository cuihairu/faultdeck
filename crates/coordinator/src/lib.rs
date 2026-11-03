use std::collections::BTreeMap;
use std::io::BufReader;
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};

use faultdeck_agent::transport::{read_frame, send_frame};
use faultdeck_core::Injection;
use faultdeck_report::{RunReport, Scenario};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CoordinatorError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("transport error: {0}")]
    Transport(#[from] faultdeck_agent::transport::TransportError),
    #[error("unknown target {0}")]
    UnknownTarget(String),
    #[error("agent not registered {0}")]
    NotRegistered(String),
}

pub type Result<T> = std::result::Result<T, CoordinatorError>;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentIdentity {
    pub id: String,
    pub capabilities: Vec<String>,
    pub last_seen_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RegisterRequest {
    pub identity: AgentIdentity,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum AgentCommand {
    Register(RegisterRequest),
    Heartbeat { id: String },
    Inject { id: String, injection: Injection },
    Rollback { id: String },
    FetchReport { id: String, scenario_hash: String },
    ListAgents,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum AgentResponse {
    Ok { message: String },
    Error { message: String },
    Registered { id: String },
    Agents { agents: Vec<AgentIdentity> },
    Report { report: RunReport },
}

#[derive(Default)]
pub struct CoordinatorState {
    pub agents: BTreeMap<String, AgentIdentity>,
    pub reports: BTreeMap<String, RunReport>,
    pub routes: BTreeMap<String, String>,
}

impl CoordinatorState {
    pub fn register(&mut self, identity: AgentIdentity) {
        self.agents.insert(identity.id.clone(), identity.clone());
    }

    pub fn heartbeat(&mut self, id: &str, now: u64) -> Option<AgentIdentity> {
        self.agents.get_mut(id).map(|a| {
            a.last_seen_ms = now;
            a.clone()
        })
    }

    pub fn store_report(&mut self, hash: String, report: RunReport) {
        self.reports.insert(hash, report);
    }

    pub fn list(&self) -> Vec<AgentIdentity> {
        self.agents.values().cloned().collect()
    }
}

pub struct Coordinator {
    pub state: Arc<Mutex<CoordinatorState>>,
}

impl Coordinator {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(CoordinatorState::default())),
        }
    }

    pub fn handle_control_command(&self, command: &AgentCommand) -> Result<AgentResponse> {
        let mut state = self.state.lock().unwrap();
        match command {
            AgentCommand::Register(reg) => {
                state.register(reg.identity.clone());
                Ok(AgentResponse::Registered {
                    id: reg.identity.id.clone(),
                })
            }
            AgentCommand::Heartbeat { id } => {
                if state.heartbeat(id, faultdeck_report::now_ms()).is_some() {
                    Ok(AgentResponse::Ok {
                        message: "heartbeat ok".into(),
                    })
                } else {
                    Err(CoordinatorError::NotRegistered(id.clone()))
                }
            }
            AgentCommand::ListAgents => Ok(AgentResponse::Agents {
                agents: state.list(),
            }),
            AgentCommand::Inject { id, injection } => {
                let scenario = Scenario {
                    name: format!("{id}-injection"),
                    injection: injection.clone(),
                };
                let hash = scenario.hash();
                let report = RunReport::new(&scenario, faultdeck_report::now_ms());
                state.store_report(hash, report);
                Ok(AgentResponse::Ok {
                    message: format!("queued for {id}"),
                })
            }
            AgentCommand::Rollback { id } => Ok(AgentResponse::Ok {
                message: format!("rollback queued for {id}"),
            }),
            AgentCommand::FetchReport { scenario_hash, .. } => {
                match state.reports.get(scenario_hash) {
                    Some(report) => Ok(AgentResponse::Report {
                        report: report.clone(),
                    }),
                    None => Ok(AgentResponse::Error {
                        message: "report not found".into(),
                    }),
                }
            }
        }
    }
}

impl Default for Coordinator {
    fn default() -> Self {
        Self::new()
    }
}

pub fn serve(state: Arc<Mutex<CoordinatorState>>, bind: &str) -> Result<()> {
    let listener = TcpListener::bind(bind)?;
    eprintln!("coordinator listening on {bind}");
    for stream in listener.incoming().flatten() {
        let state = state.clone();
        std::thread::spawn(move || {
            let _ = handle_stream(state, stream);
        });
    }
    Ok(())
}

fn handle_stream(state: Arc<Mutex<CoordinatorState>>, stream: TcpStream) -> Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut writer = stream;
    while let Ok(cmd) = read_frame(&mut reader) {
        let command: AgentCommand = cmd;
        let coordinator = Coordinator {
            state: state.clone(),
        };
        let response = coordinator.handle_control_command(&command);
        let payload = match response {
            Ok(resp) => resp,
            Err(e) => AgentResponse::Error {
                message: e.to_string(),
            },
        };
        if send_frame(&mut writer, &payload).is_err() {
            break;
        }
    }
    Ok(())
}
