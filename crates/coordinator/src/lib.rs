use std::collections::{BTreeMap, VecDeque};
use std::io::BufReader;
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, SyncSender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use faultdeck_agent::link::{LinkMessage, RegisterEnvelope};
use faultdeck_agent::transport::{read_frame, send_frame};
use faultdeck_agent::{constant_time_eq, Request as AgentRequest, Response as AgentResponseMsg};
use faultdeck_core::{validate_injection, Injection};
use faultdeck_report::{FaultEvent, RunReport, Scenario};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// How long an agent may go without a heartbeat (or a live link) before it is
/// considered offline and dropped from the available list.
pub const HEARTBEAT_TIMEOUT_MS: u64 = 30_000;

/// How long the controller waits for an agent to acknowledge a dispatched
/// command before reporting a timeout. The command stays queued and is
/// delivered on reconnect.
pub const DEFAULT_DISPATCH_TIMEOUT: Duration = Duration::from_secs(5);

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
    #[error("agent {0} has no active connection")]
    NotConnected(String),
    #[error("unauthorized: missing or invalid token")]
    Unauthorized,
    #[error("invalid injection: {0}")]
    InvalidInjection(String),
}

pub type Result<T> = std::result::Result<T, CoordinatorError>;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentIdentity {
    pub id: String,
    pub capabilities: Vec<String>,
    pub last_seen_ms: u64,
    /// Computed at list time: true while the agent has a live link or a fresh
    /// heartbeat. `#[serde(default)]` keeps agent-sent registration JSON (which
    /// has no such field) decoding cleanly.
    #[serde(default)]
    pub online: bool,
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
    Ok {
        message: String,
    },
    Error {
        message: String,
    },
    Registered {
        id: String,
    },
    /// Command accepted into the queue; delivered when the agent connects.
    Queued {
        agent_id: String,
        pending: u64,
    },
    /// The dispatch wait expired before the agent replied. This is *not* an
    /// execution failure: the command stays in flight (or queued) and its
    /// eventual result is still applied and folded into the archived report.
    /// Controllers must not assume the fault was not applied; check the
    /// report or `list_agents` before re-injecting.
    TimedOut {
        agent_id: String,
    },
    /// Agent list with per-agent online status plus the ids of online agents.
    Agents {
        agents: Vec<AgentIdentity>,
        available: Vec<String>,
    },
    Report {
        report: RunReport,
    },
}

/// An outcome routed back from the agent link reader to whoever submitted the
/// command (the controller session waiting on `tx`, or nobody for commands
/// drained on reconnect). Report and applied-state updates happen in the
/// reader before the outcome is delivered.
struct DispatchOutcome {
    response: AgentResponseMsg,
}

/// A command waiting in an agent's queue (or in flight to it). Internal to the
/// coordinator; `CoordinatorState::queues` exposes it only for inspection.
pub struct PendingCommand {
    pub(crate) request: AgentRequest,
    pub(crate) scenario_hash: Option<String>,
    pub(crate) is_rollback: bool,
    pub(crate) tx: Option<SyncSender<DispatchOutcome>>,
}

/// Bookkeeping for one dispatched (in-flight) command, so the link reader can
/// route the agent's `Result` frame back to the waiting submitter and update
/// the archived report.
pub struct PendingDispatch {
    pub(crate) tx: Option<SyncSender<DispatchOutcome>>,
    pub(crate) scenario_hash: Option<String>,
    pub(crate) is_rollback: bool,
}

/// The coordinator side of one agent link (the agent's reverse-dialed TCP
/// connection). Writes to the shared writer are serialized by the mutex;
/// `pending` maps an in-flight request id to its routing bookkeeping.
pub struct AgentLink {
    /// Shared write half of the agent's reverse channel. One mutex guards all
    /// writes to the socket (registration ack, dispatcher `Dispatch` frames),
    /// so concurrent writers cannot interleave at the byte level.
    pub writer: Arc<Mutex<TcpStream>>,
    pub pending: Mutex<BTreeMap<u64, PendingDispatch>>,
    pub next_request_id: AtomicU64,
    pub connected_at_ms: u64,
}

impl AgentLink {
    fn new(writer: Arc<Mutex<TcpStream>>, connected_at_ms: u64) -> Self {
        Self {
            writer,
            pending: Mutex::new(BTreeMap::new()),
            next_request_id: AtomicU64::new(0),
            connected_at_ms,
        }
    }
}

#[derive(Default)]
pub struct CoordinatorState {
    pub agents: BTreeMap<String, AgentIdentity>,
    pub reports: BTreeMap<String, RunReport>,
    /// Live agent links: agent id -> link handle. A present entry means the
    /// agent dialed in and its reverse channel can carry commands (solves
    /// double-NAT without inbound reachability).
    pub links: BTreeMap<String, Arc<AgentLink>>,
    /// Per-agent FIFO of commands awaiting dispatch.
    pub queues: BTreeMap<String, VecDeque<PendingCommand>>,
    /// Agent id -> scenario hash of the fault currently applied (set when an
    /// inject is acknowledged, cleared when its rollback is).
    pub applied: BTreeMap<String, String>,
    /// Scenario hash -> agent id whose injection produced it. Gates
    /// `FetchReport` to the owning agent (audit 2026-11-11 P2).
    pub report_owner: BTreeMap<String, String>,
}

impl CoordinatorState {
    pub fn register(&mut self, identity: AgentIdentity) {
        let mut identity = identity;
        // Registration is metadata; liveness is decided by links/heartbeats.
        identity.online = false;
        self.agents.insert(identity.id.clone(), identity);
    }

    pub fn heartbeat(&mut self, id: &str, now: u64) -> Option<AgentIdentity> {
        self.agents.get_mut(id).map(|a| {
            a.last_seen_ms = now;
            a.clone()
        })
    }

    /// Store a run report and remember which agent's injection produced it, so
    /// `FetchReport` can gate reads to the owning agent.
    pub fn store_report(&mut self, hash: String, report: RunReport, agent_id: &str) {
        self.report_owner.insert(hash.clone(), agent_id.to_string());
        self.reports.insert(hash, report);
    }

    /// All agents annotated with online status. An agent is online while it
    /// holds a live link OR its last heartbeat is within `heartbeat_timeout_ms`.
    pub fn list(&self, now_ms: u64, heartbeat_timeout_ms: u64) -> Vec<AgentIdentity> {
        self.agents
            .values()
            .map(|a| {
                let mut a = a.clone();
                a.online = self.is_online(&a.id, now_ms, heartbeat_timeout_ms);
                a
            })
            .collect()
    }

    /// Ids of agents currently online (live link or fresh heartbeat).
    pub fn available(&self, now_ms: u64, heartbeat_timeout_ms: u64) -> Vec<String> {
        self.list(now_ms, heartbeat_timeout_ms)
            .into_iter()
            .filter(|a| a.online)
            .map(|a| a.id)
            .collect()
    }

    pub fn is_online(&self, id: &str, now_ms: u64, heartbeat_timeout_ms: u64) -> bool {
        self.links.contains_key(id)
            || self
                .agents
                .get(id)
                .map(|a| now_ms.saturating_sub(a.last_seen_ms) <= heartbeat_timeout_ms)
                .unwrap_or(false)
    }

    fn enqueue(&mut self, agent_id: &str, command: PendingCommand) -> u64 {
        let queue = self.queues.entry(agent_id.to_string()).or_default();
        let pending = queue.len() as u64 + 1;
        queue.push_back(command);
        pending
    }
}

pub struct Coordinator {
    pub state: Arc<Mutex<CoordinatorState>>,
    pub token: Option<String>,
    pub dispatch_timeout: Duration,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WireMessage {
    pub token: Option<String>,
    pub command: AgentCommand,
}

impl Coordinator {
    pub fn new() -> Self {
        Self::with_token(None)
    }

    pub fn with_token(token: Option<String>) -> Self {
        Self {
            state: Arc::new(Mutex::new(CoordinatorState::default())),
            token,
            dispatch_timeout: DEFAULT_DISPATCH_TIMEOUT,
        }
    }

    pub fn authorize(&self, token: Option<&str>) -> Result<()> {
        match &self.token {
            None => Ok(()),
            // Constant-time comparison to match faultdeck_agent::token_ok
            // (commit 4dbd049); a plain == would leak the token length and
            // byte positions via timing, and was inconsistent with the agent.
            Some(expected)
                if token
                    .map(|t| constant_time_eq(t.as_bytes(), expected.as_bytes()))
                    .unwrap_or(false) =>
            {
                Ok(())
            }
            _ => Err(CoordinatorError::Unauthorized),
        }
    }

    pub fn handle_control_command(
        &self,
        command: &AgentCommand,
        request_token: Option<&str>,
    ) -> Result<AgentResponse> {
        self.authorize(request_token)?;
        match command {
            AgentCommand::Register(reg) => {
                self.state.lock().unwrap().register(reg.identity.clone());
                Ok(AgentResponse::Registered {
                    id: reg.identity.id.clone(),
                })
            }
            AgentCommand::Heartbeat { id } => {
                let mut state = self.state.lock().unwrap();
                // A heartbeat over the control channel cannot by itself prove
                // liveness: only an agent holding a live reverse link is
                // genuinely online. Without this gate, any reachable controller
                // could keep a dead agent listed as online by replaying
                // heartbeats (audit 2026-11-11 P2).
                if !state.agents.contains_key(id) {
                    return Err(CoordinatorError::NotRegistered(id.clone()));
                }
                if !state.links.contains_key(id) {
                    return Err(CoordinatorError::NotConnected(id.clone()));
                }
                state.heartbeat(id, faultdeck_report::now_ms());
                Ok(AgentResponse::Ok {
                    message: "heartbeat ok".into(),
                })
            }
            AgentCommand::ListAgents => {
                let state = self.state.lock().unwrap();
                Ok(AgentResponse::Agents {
                    agents: state.list(faultdeck_report::now_ms(), HEARTBEAT_TIMEOUT_MS),
                    available: state.available(faultdeck_report::now_ms(), HEARTBEAT_TIMEOUT_MS),
                })
            }
            AgentCommand::Inject { id, injection } => {
                if !self.state.lock().unwrap().agents.contains_key(id) {
                    return Err(CoordinatorError::NotRegistered(id.clone()));
                }
                validate_injection(injection)
                    .map_err(|e| CoordinatorError::InvalidInjection(e.to_string()))?;
                let scenario = Scenario {
                    name: format!("{id}-injection"),
                    injection: injection.clone(),
                };
                let hash = scenario.hash();
                let report = RunReport::new(&scenario, faultdeck_report::now_ms());
                self.state
                    .lock()
                    .unwrap()
                    .store_report(hash.clone(), report, id);
                self.submit_control(
                    id,
                    AgentRequest::Inject(injection.clone()),
                    Some(hash),
                    false,
                )
            }
            AgentCommand::Rollback { id } => {
                if !self.state.lock().unwrap().agents.contains_key(id) {
                    return Err(CoordinatorError::NotRegistered(id.clone()));
                }
                let applied_hash = self.state.lock().unwrap().applied.get(id).cloned();
                self.submit_control(id, AgentRequest::Rollback, applied_hash, true)
            }
            AgentCommand::FetchReport { id, scenario_hash } => {
                let state = self.state.lock().unwrap();
                // Ownership: only the agent whose injection produced the
                // report may fetch it. A non-existent hash and a report owned
                // by a different agent both answer "not found" so reports
                // cannot be enumerated by probing hashes across agent ids
                // (audit 2026-11-11 P2).
                let owned = state.report_owner.get(scenario_hash) == Some(id);
                let report = if owned {
                    state.reports.get(scenario_hash).cloned()
                } else {
                    None
                };
                match report {
                    Some(report) => Ok(AgentResponse::Report { report }),
                    None => Ok(AgentResponse::Error {
                        message: "report not found".into(),
                    }),
                }
            }
        }
    }

    /// Enqueue a command for `agent_id` and, when the agent has a live link,
    /// wait for its execution result (the dispatcher drains the queue and the
    /// link reader routes the outcome back here).
    fn submit_control(
        &self,
        agent_id: &str,
        request: AgentRequest,
        scenario_hash: Option<String>,
        is_rollback: bool,
    ) -> Result<AgentResponse> {
        let (tx, rx) = mpsc::sync_channel(1);
        let online = {
            let mut state = self.state.lock().unwrap();
            if !state.agents.contains_key(agent_id) {
                return Err(CoordinatorError::NotRegistered(agent_id.into()));
            }
            let online = state.links.contains_key(agent_id);
            let pending = state.enqueue(
                agent_id,
                PendingCommand {
                    request,
                    scenario_hash,
                    is_rollback,
                    tx: if online { Some(tx) } else { None },
                },
            );
            if !online {
                return Ok(AgentResponse::Queued {
                    agent_id: agent_id.into(),
                    pending,
                });
            }
            online
        };
        let _ = online;
        match rx.recv_timeout(self.dispatch_timeout) {
            Ok(outcome) => Ok(match outcome.response {
                AgentResponseMsg::Ok { message } => AgentResponse::Ok { message },
                AgentResponseMsg::Error { message } => AgentResponse::Error { message },
            }),
            // Timeout is an *outcome-unknown* answer, not an error: the
            // dispatcher already handed the command to the live link, so the
            // agent may still execute it and the late result will be applied
            // and recorded by `route_result`. Surfacing it as an error would
            // invite double injection or an abandoned rollback.
            Err(_) => Ok(AgentResponse::TimedOut {
                agent_id: agent_id.into(),
            }),
        }
    }
}

impl Default for Coordinator {
    fn default() -> Self {
        Self::new()
    }
}

pub fn serve(state: Arc<Mutex<CoordinatorState>>, bind: &str) -> Result<()> {
    serve_with_token(state, bind, None)
}

pub fn serve_with_token(
    state: Arc<Mutex<CoordinatorState>>,
    bind: &str,
    token: Option<String>,
) -> Result<()> {
    let listener = TcpListener::bind(bind)?;
    serve_on_with_token(state, listener, token)
}

pub fn serve_on_with_token(
    state: Arc<Mutex<CoordinatorState>>,
    listener: TcpListener,
    token: Option<String>,
) -> Result<()> {
    serve_on_with_options(state, listener, token, DEFAULT_DISPATCH_TIMEOUT)
}

pub fn serve_on_with_options(
    state: Arc<Mutex<CoordinatorState>>,
    listener: TcpListener,
    token: Option<String>,
    dispatch_timeout: Duration,
) -> Result<()> {
    eprintln!("coordinator listening on {}", listener.local_addr()?);
    for stream in listener.incoming().flatten() {
        let state = state.clone();
        let token = token.clone();
        std::thread::spawn(move || {
            let _ = handle_stream_with_options(state, stream, token, dispatch_timeout);
        });
    }
    Ok(())
}

/// Public entry point for tests and one-shot TCP handlers.
pub fn handle_connection(
    state: Arc<Mutex<CoordinatorState>>,
    stream: TcpStream,
    token: Option<String>,
) -> Result<()> {
    handle_connection_with(state, stream, token, DEFAULT_DISPATCH_TIMEOUT)
}

pub fn handle_connection_with(
    state: Arc<Mutex<CoordinatorState>>,
    stream: TcpStream,
    token: Option<String>,
    dispatch_timeout: Duration,
) -> Result<()> {
    handle_stream_with_options(state, stream, token, dispatch_timeout)
}

fn handle_stream_with_options(
    state: Arc<Mutex<CoordinatorState>>,
    stream: TcpStream,
    token: Option<String>,
    dispatch_timeout: Duration,
) -> Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let writer = Arc::new(Mutex::new(stream));

    let first_line = faultdeck_agent::read_line_limited(
        &mut reader,
        faultdeck_agent::transport::MAX_FRAME_BYTES,
    )?;
    // An agent link announces itself with a RegisterEnvelope (id/capabilities,
    // no `command` key); a controller session starts with a WireMessage. The
    // two shapes are disjoint, so parse disambiguates them.
    if let Ok(envelope) = serde_json::from_str::<RegisterEnvelope>(&first_line) {
        return handle_agent_link(state, token, envelope, reader, writer);
    }
    let mut current: WireMessage = serde_json::from_str(&first_line)?;
    loop {
        let coordinator = Coordinator {
            state: state.clone(),
            token: token.clone(),
            dispatch_timeout,
        };
        let response =
            coordinator.handle_control_command(&current.command, current.token.as_deref());
        let payload = match response {
            Ok(resp) => resp,
            Err(e) => AgentResponse::Error {
                message: e.to_string(),
            },
        };
        if send_frame(&mut *writer.lock().unwrap(), &payload).is_err() {
            break;
        }
        match read_frame::<_, WireMessage>(&mut reader) {
            Ok(message) => current = message,
            Err(_) => break,
        }
    }
    Ok(())
}

/// Bind an agent's reverse-dialed connection as its link, then pump the link:
/// route `Result` frames back to waiting submitters (updating the archived
/// report) and record heartbeats until the socket drops.
fn handle_agent_link(
    state: Arc<Mutex<CoordinatorState>>,
    token: Option<String>,
    envelope: RegisterEnvelope,
    mut reader: BufReader<std::net::TcpStream>,
    writer: Arc<Mutex<TcpStream>>,
) -> Result<()> {
    let coordinator = Coordinator {
        state: state.clone(),
        token: token.clone(),
        dispatch_timeout: DEFAULT_DISPATCH_TIMEOUT,
    };
    // A rejected registration must get an explicit error frame, not a bare
    // connection drop: otherwise the agent cannot tell a permanent rejection
    // (bad token) from a transient drop and reconnects forever — a storm
    // (audit 2026-11-11 P2).
    if let Err(e) = coordinator.authorize(envelope.token.as_deref()) {
        let _ = send_frame(
            &mut *writer.lock().unwrap(),
            &AgentResponse::Error {
                message: e.to_string(),
            },
        );
        return Err(e);
    }

    let now = faultdeck_report::now_ms();
    let identity = AgentIdentity {
        id: envelope.id.clone(),
        capabilities: envelope.capabilities.clone(),
        last_seen_ms: now,
        online: false,
    };
    let link = Arc::new(AgentLink::new(writer.clone(), now));
    {
        let mut state = state.lock().unwrap();
        state.register(identity.clone());
        state.links.insert(envelope.id.clone(), link.clone());
        // Ensure a queue exists so the dispatcher has a target; pre-existing
        // queued commands (enqueued while the agent was offline) are drained.
        state.queues.entry(envelope.id.clone()).or_default();
    }
    // Ack the registration over the same reverse channel.
    let ack = AgentResponse::Registered {
        id: envelope.id.clone(),
    };
    if send_frame(&mut *writer.lock().unwrap(), &ack).is_err() {
        // Registration failed to ack; drop the link so the next register wins.
        remove_link_if_current(&state, &envelope.id, &link);
        return Ok(());
    }
    spawn_dispatcher(state.clone(), envelope.id.clone(), link.clone());

    let result = pump_link(&state, &envelope.id, &link, &mut reader);
    remove_link_if_current(&state, &envelope.id, &link);
    result
}

fn pump_link(
    state: &Arc<Mutex<CoordinatorState>>,
    agent_id: &str,
    link: &Arc<AgentLink>,
    reader: &mut BufReader<std::net::TcpStream>,
) -> Result<()> {
    loop {
        match read_frame::<_, LinkMessage>(reader) {
            Ok(LinkMessage::Result {
                request_id,
                response,
            }) => {
                route_result(state, agent_id, link, request_id, response);
            }
            Ok(LinkMessage::Heartbeat { id }) => {
                state
                    .lock()
                    .unwrap()
                    .heartbeat(&id, faultdeck_report::now_ms());
            }
            Ok(_) => {}
            Err(_) => break,
        }
    }
    Ok(())
}

/// Route an agent `Result` frame to the waiting submitter and fold it into the
/// archived report (the single source of truth for the run).
fn route_result(
    state: &Arc<Mutex<CoordinatorState>>,
    agent_id: &str,
    link: &Arc<AgentLink>,
    request_id: u64,
    response: AgentResponseMsg,
) {
    let pending = link.pending.lock().unwrap().remove(&request_id);
    let Some(pending) = pending else { return };
    let now = faultdeck_report::now_ms();
    let mut st = state.lock().unwrap();
    if let Some(hash) = &pending.scenario_hash {
        if let Some(report) = st.reports.get_mut(hash) {
            report.events.push(FaultEvent {
                timestamp_ms: now,
                kind: if pending.is_rollback {
                    "rollback".into()
                } else {
                    "dispatch".into()
                },
                message: response_message(&response),
            });
            if pending.is_rollback && is_ok(&response) {
                report.rolled_back_at_ms = Some(now);
            }
        }
    }
    if is_ok(&response) {
        if pending.is_rollback {
            st.applied.remove(agent_id);
        } else if let Some(hash) = &pending.scenario_hash {
            st.applied.insert(agent_id.to_string(), hash.clone());
        }
    }
    drop(st);
    if let Some(tx) = &pending.tx {
        let _ = tx.send(DispatchOutcome { response });
    }
}

fn is_ok(response: &AgentResponseMsg) -> bool {
    matches!(response, AgentResponseMsg::Ok { .. })
}

fn response_message(response: &AgentResponseMsg) -> String {
    match response {
        AgentResponseMsg::Ok { message } => message.clone(),
        AgentResponseMsg::Error { message } => format!("error: {message}"),
    }
}

/// Drop the link if it is still the current one for `agent_id` (a newer
/// registration must not be clobbered by an old connection's teardown). Any
/// in-flight dispatch is failed so waiting submitters unblock.
fn remove_link_if_current(
    state: &Arc<Mutex<CoordinatorState>>,
    agent_id: &str,
    link: &Arc<AgentLink>,
) {
    let mut st = state.lock().unwrap();
    let is_current = st
        .links
        .get(agent_id)
        .map(|current| Arc::ptr_eq(current, link))
        .unwrap_or(false);
    if is_current {
        st.links.remove(agent_id);
    }
    let pending = std::mem::take(&mut *link.pending.lock().unwrap());
    for (_, pd) in pending {
        if let Some(tx) = pd.tx {
            let _ = tx.send(DispatchOutcome {
                response: AgentResponseMsg::Error {
                    message: "agent disconnected before replying".into(),
                },
            });
        }
    }
}

/// One per link generation: drains the agent's queue, writes `Dispatch` frames
/// down the reverse channel and lets the link reader route the outcome back.
fn spawn_dispatcher(state: Arc<Mutex<CoordinatorState>>, agent_id: String, link: Arc<AgentLink>) {
    std::thread::spawn(move || loop {
        // Pop the next queued command for this agent, or wait briefly and
        // re-check. The lock is released between checks so enqueues (from the
        // controller) proceed. Exit when this link is no longer the current
        // one (dropped or replaced by a reconnect) — queued commands survive
        // for the next generation's dispatcher.
        let command = loop {
            let mut st = state.lock().unwrap();
            let current = st
                .links
                .get(&agent_id)
                .map(|l| Arc::ptr_eq(l, &link))
                .unwrap_or(false);
            if !current {
                return;
            }
            if let Some(command) = st.queues.get_mut(&agent_id).and_then(|q| q.pop_front()) {
                break command;
            }
            drop(st);
            std::thread::sleep(Duration::from_millis(50));
        };
        // For a rollback, resolve the applied scenario hash at dispatch time
        // (the inject may have completed only after the command was enqueued).
        let scenario_hash = if command.is_rollback {
            state.lock().unwrap().applied.get(&agent_id).cloned()
        } else {
            command.scenario_hash.clone()
        };
        let request_id = link.next_request_id.fetch_add(1, Ordering::SeqCst);
        link.pending.lock().unwrap().insert(
            request_id,
            PendingDispatch {
                tx: command.tx,
                scenario_hash,
                is_rollback: command.is_rollback,
            },
        );
        let message = LinkMessage::Dispatch {
            request_id,
            request: command.request,
        };
        let sent = send_frame(&mut *link.writer.lock().unwrap(), &message);
        if sent.is_err() {
            // Socket died mid-write: fail the pending entry so the submitter
            // (if any) unblocks instead of waiting for the timeout.
            if let Some(pd) = link.pending.lock().unwrap().remove(&request_id) {
                if let Some(tx) = pd.tx {
                    let _ = tx.send(DispatchOutcome {
                        response: AgentResponseMsg::Error {
                            message: "dispatch write failed".into(),
                        },
                    });
                }
            }
        }
    });
}
