//! FaultDeck MCP server — the CLI's existing injection chain, exposed to AI
//! agents over stdio (official `rmcp` SDK).
//!
//! Discipline: **zero second injection implementation here.** The tools speak
//! the same coordinator wire protocol as `faultdeck hub` (`WireMessage` /
//! `AgentCommand` over the shared transport framing). Validation stays in the
//! coordinator (`faultdeck_core::validate_injection`), and application stays
//! in the agent (`tc netem` / `iptables` backends). The MCP layer never
//! guesses backend availability (e.g. Windows): capabilities come from each
//! agent's own registration via `list_targets`, and every failure passes
//! through verbatim as a tool-level error (`is_error: true`).
//!
//! Honest capability surface (README "Shipped fault types"): latency / loss /
//! disconnect only. Blackhole and bandwidth are not implemented anywhere in
//! this repo, so no tool or parameter pretends otherwise. Game scenario
//! templates are not shipped either, so `run_scenario` runs exactly what the
//! CLI supports — a named scenario over the same inject chain — nothing more.
//!
//! `apply_fault` is destructive: it requires an explicit target agent id
//! (from `list_targets`) and its tool description says so.
//!
//! stdio transport: stdout carries only MCP frames; diagnostics are not
//! printed (silent discipline, same as the sandtable MCP server).

use std::io::{BufReader, BufWriter};
use std::net::TcpStream;
use std::str::FromStr;
use std::time::Duration;

use clap::Parser;
use faultdeck_agent::transport::{read_frame, send_frame};
use faultdeck_coordinator::{AgentCommand, AgentResponse, WireMessage};
use faultdeck_core::{DisconnectSpec, FaultSpec, Injection, LatencySpec, LossSpec, Target};
use faultdeck_report::Scenario;
use ipnet::IpNet;
use rmcp::{
    handler::server::wrapper::Parameters,
    model::{CallToolResult, ContentBlock},
    schemars, tool, tool_router, ErrorData, ServiceExt,
};
use serde::Deserialize;
use serde_json::{json, Value};

/// Where the coordinator lives. The MCP server drives the hub chain only —
/// the CLI's `remote` (direct-agent) mode stays CLI-only, which keeps this
/// surface honest: `list_targets` has something real to enumerate.
#[derive(Debug, Clone)]
pub struct HubConfig {
    pub host: String,
    pub port: u16,
    pub token: Option<String>,
}

impl HubConfig {
    pub fn addr(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

/// One command to the coordinator and back — the same framing, types and
/// timeout discipline as the CLI's `hub_send` (crates/cli/src/main.rs).
/// This is transport glue, not an injection path: the fault is applied by
/// the agent's own backends exactly as with the CLI.
fn hub_send(config: &HubConfig, command: AgentCommand) -> Result<AgentResponse, String> {
    let stream = TcpStream::connect((config.host.as_str(), config.port))
        .map_err(|e| format!("coordinator unreachable at {}: {e}", config.addr()))?;
    let timeout_secs: u64 = std::env::var("FAULTDECK_TIMEOUT_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(5);
    stream
        .set_read_timeout(Some(Duration::from_secs(timeout_secs)))
        .map_err(|e| e.to_string())?;
    stream
        .set_write_timeout(Some(Duration::from_secs(timeout_secs)))
        .map_err(|e| e.to_string())?;
    let mut writer = BufWriter::new(stream.try_clone().map_err(|e| e.to_string())?);
    send_frame(
        &mut writer,
        &WireMessage {
            token: config.token.clone(),
            command,
        },
    )
    .map_err(|e| e.to_string())?;
    let mut reader = BufReader::new(stream);
    read_frame::<_, AgentResponse>(&mut reader).map_err(|e| e.to_string())
}

/// Map a coordinator response onto a focused JSON value; an `Error` response
/// becomes `Err` so it lands on the tool-level error channel verbatim.
fn response_to_value(resp: AgentResponse) -> Result<Value, String> {
    match resp {
        AgentResponse::Error { message } => Err(message),
        AgentResponse::Ok { message } => Ok(json!({ "status": "ok", "message": message })),
        AgentResponse::Registered { id } => Ok(json!({ "status": "registered", "agent": id })),
        AgentResponse::Queued { agent_id, pending } => Ok(json!({
            "status": "queued",
            "agent": agent_id,
            "pending": pending,
            "note": "agent offline; the command is saved and delivered when it reconnects",
        })),
        AgentResponse::TimedOut { agent_id } => Ok(json!({
            "status": "timed_out",
            "agent": agent_id,
            "note": "the command may still be in flight; check read_report or list_targets before re-applying",
        })),
        AgentResponse::Agents { agents, available } => Ok(json!({
            "agents": agents
                .iter()
                .map(|a| json!({
                    "id": a.id,
                    "online": a.online,
                    "capabilities": a.capabilities,
                    "last_seen_ms": a.last_seen_ms,
                }))
                .collect::<Vec<_>>(),
            "available": available,
        })),
        AgentResponse::Report { report } => {
            serde_json::to_value(report).map_err(|e| format!("report serialization failed: {e}"))
        }
    }
}

/// Build an `Injection` from tool arguments with the same semantics as the
/// CLI's `build_injection` (crates/cli/src/main.rs). Range validation is the
/// coordinator's job (`faultdeck_core::validate_injection`); only the
/// required-parameter mapping happens here. Shipped kinds only — anything
/// else is rejected by name.
fn build_injection(
    fault: &str,
    destination: &str,
    port: Option<u16>,
    milliseconds: Option<u64>,
    percent: Option<f64>,
) -> Result<Injection, String> {
    let net = IpNet::from_str(destination)
        .map_err(|e| format!("invalid destination CIDR {destination:?}: {e}"))?;
    let target = match port {
        Some(p) => Target::destination_port(net, p),
        None => Target::destination(net),
    };
    let spec = match fault {
        "latency" => FaultSpec::Latency(LatencySpec {
            milliseconds: milliseconds.ok_or_else(|| {
                "fault latency requires milliseconds (0 < ms <= 86400000)".to_string()
            })?,
        }),
        "loss" => FaultSpec::Loss(LossSpec {
            percent: percent
                .ok_or_else(|| "fault loss requires percent (0 < percent <= 100)".to_string())?,
        }),
        "disconnect" => FaultSpec::Disconnect(DisconnectSpec {
            all_traffic: port.is_none(),
        }),
        other => {
            return Err(format!(
                "unknown fault kind {other:?}; shipped kinds: latency / loss / disconnect \
                 (blackhole and bandwidth are not implemented)"
            ))
        }
    };
    Ok(Injection {
        fault: spec,
        target,
    })
}

// —— native layer (Result<Value, String>, testable without the MCP protocol) ——

/// One apply_fault request as delivered by the tool layer: the fault kind,
/// its destination, and the per-kind parameters (only the kind's own
/// parameter is required).
#[derive(Debug, Default, Clone)]
pub struct FaultRequest {
    pub fault: String,
    pub destination: String,
    pub port: Option<u16>,
    pub milliseconds: Option<u64>,
    pub percent: Option<f64>,
    pub scenario_name: Option<String>,
}

fn list_targets_native(config: &HubConfig) -> Result<Value, String> {
    response_to_value(hub_send(config, AgentCommand::ListAgents)?)
}

fn apply_fault_native(
    config: &HubConfig,
    agent: &str,
    req: &FaultRequest,
) -> Result<Value, String> {
    let injection = build_injection(
        &req.fault,
        &req.destination,
        req.port,
        req.milliseconds,
        req.percent,
    )?;
    let mut out = response_to_value(hub_send(
        config,
        AgentCommand::Inject {
            id: agent.to_string(),
            injection,
            scenario_name: req.scenario_name.clone(),
        },
    )?)?;
    if let Some(obj) = out.as_object_mut() {
        obj.insert("agent".into(), json!(agent));
    }
    Ok(out)
}

fn clear_fault_native(config: &HubConfig, agent: &str) -> Result<Value, String> {
    let mut out = response_to_value(hub_send(
        config,
        AgentCommand::Rollback {
            id: agent.to_string(),
        },
    )?)?;
    if let Some(obj) = out.as_object_mut() {
        obj.insert("agent".into(), json!(agent));
    }
    Ok(out)
}

/// Apply a named scenario (the report crate's `Scenario` JSON shape) through
/// the same inject chain as `apply_fault`, and attach the stable scenario
/// hash so `read_report` can follow up. Game scenario templates are not
/// shipped; this runs what the CLI supports, nothing more.
fn run_scenario_native(
    config: &HubConfig,
    agent: &str,
    scenario_json: &str,
) -> Result<Value, String> {
    let scenario: Scenario = serde_json::from_str(scenario_json).map_err(|e| {
        format!(
            "invalid scenario JSON (expected {{\"name\": ..., \"injection\": \
             {{\"fault\": ..., \"target\": ...}}}}): {e}"
        )
    })?;
    let hash = scenario.hash();
    let mut out = response_to_value(hub_send(
        config,
        AgentCommand::Inject {
            id: agent.to_string(),
            injection: scenario.injection.clone(),
            scenario_name: Some(scenario.name.clone()),
        },
    )?)?;
    if let Some(obj) = out.as_object_mut() {
        obj.insert("agent".into(), json!(agent));
        obj.insert("scenario_name".into(), json!(scenario.name));
        obj.insert("scenario_hash".into(), json!(hash));
    }
    Ok(out)
}

fn read_report_native(
    config: &HubConfig,
    agent: &str,
    scenario_hash: &str,
) -> Result<Value, String> {
    response_to_value(hub_send(
        config,
        AgentCommand::FetchReport {
            id: agent.to_string(),
            scenario_hash: scenario_hash.to_string(),
        },
    )?)
}

// —— MCP tool layer (parameter structs + rmcp macros; field docs are the
//    inputSchema descriptions) ——

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ApplyFaultParams {
    /// Target agent id from list_targets (required; faults are never applied without an explicit target)
    pub agent: String,
    /// Shipped kinds only: latency | loss | disconnect (blackhole and bandwidth are not implemented)
    pub fault: String,
    /// Destination CIDR the fault applies to, e.g. "10.0.0.0/24" (IPv4; IPv6 is rejected by the coordinator)
    pub destination: String,
    /// Optional port; for disconnect, presence switches to a port-targeted tcp+udp drop instead of dropping all traffic to the destination
    #[serde(default)]
    #[schemars(default)]
    pub port: Option<u16>,
    /// Required for latency (0 < ms <= 86400000)
    #[serde(default)]
    #[schemars(default)]
    pub milliseconds: Option<u64>,
    /// Required for loss (0 < percent <= 100)
    #[serde(default)]
    #[schemars(default)]
    pub percent: Option<f64>,
    /// Optional scenario name recorded in the archived report
    #[serde(default)]
    #[schemars(default)]
    pub scenario_name: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ClearFaultParams {
    /// Target agent id from list_targets
    pub agent: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct RunScenarioParams {
    /// Target agent id from list_targets
    pub agent: String,
    /// Scenario JSON, same shape as the report's Scenario:
    /// {"name": "...", "injection": {"fault": {"kind": "latency", "milliseconds": 100}, "target": {"source": null, "destination": "10.0.0.0/24", "destination_port": null}}}
    pub scenario: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ReadReportParams {
    /// Owning agent id — reports are readable only by the agent whose injection produced them
    pub agent: String,
    /// Stable scenario hash (returned by run_scenario / listed in reports)
    pub scenario_hash: String,
}

fn to_tool_result(v: Result<Value, String>) -> Result<CallToolResult, ErrorData> {
    match v {
        Ok(v) => {
            // Serialization of an acyclic Value cannot actually fail; failure
            // means a broken internal invariant, so surface it honestly on
            // the tool-level error channel rather than returning an empty
            // "success".
            let text = serde_json::to_string(&v).map_err(|e| {
                ErrorData::internal_error(format!("result serialization failed: {e}"), None)
            })?;
            Ok(CallToolResult::success(vec![ContentBlock::text(text)]))
        }
        // Coordinator / validation errors pass through verbatim
        // (tool-level error, never rewritten at the shell layer).
        Err(e) => Ok(CallToolResult::error(vec![ContentBlock::text(e)])),
    }
}

#[derive(Debug, Clone)]
pub struct FaultdeckMcp {
    config: HubConfig,
}

impl FaultdeckMcp {
    pub fn new(host: String, port: u16, token: Option<String>) -> Self {
        Self {
            config: HubConfig { host, port, token },
        }
    }
}

#[tool_router(server_handler)]
impl FaultdeckMcp {
    #[tool(
        description = "List fault-injection targets: agents registered with the FaultDeck coordinator, with live online status and the capabilities each agent itself reported (netem / firewall). Backend availability is whatever the agents report here — the MCP layer never guesses it."
    )]
    async fn list_targets(&self) -> Result<CallToolResult, ErrorData> {
        let config = self.config.clone();
        let out: Result<Value, String> =
            tokio::task::spawn_blocking(move || list_targets_native(&config))
                .await
                .unwrap_or_else(|e| Err(format!("tool task failed: {e}")));
        to_tool_result(out)
    }

    #[tool(
        description = "Apply a network fault to an explicit target agent through the FaultDeck coordinator — the same chain as `faultdeck hub inject` (coordinator validates, agent applies via tc netem or iptables). DESTRUCTIVE: requires an explicit target agent id from list_targets; the fault degrades or drops real traffic on that host until clear_fault. Shipped kinds: latency (milliseconds), loss (percent), disconnect (iptables DROP; with port: tcp+udp on that port, otherwise all traffic to the destination). blackhole and bandwidth are not implemented and are rejected by name. An offline target queues the command; an unknown result is reported as timed_out — check read_report or list_targets before re-applying."
    )]
    async fn apply_fault(
        &self,
        Parameters(ApplyFaultParams {
            agent,
            fault,
            destination,
            port,
            milliseconds,
            percent,
            scenario_name,
        }): Parameters<ApplyFaultParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let config = self.config.clone();
        let out: Result<Value, String> = tokio::task::spawn_blocking(move || {
            apply_fault_native(
                &config,
                &agent,
                &FaultRequest {
                    fault,
                    destination,
                    port,
                    milliseconds,
                    percent,
                    scenario_name,
                },
            )
        })
        .await
        .unwrap_or_else(|e| Err(format!("tool task failed: {e}")));
        to_tool_result(out)
    }

    #[tool(
        description = "Roll back the fault currently applied to an explicit target agent (same chain as `faultdeck hub rollback`). Requires the target agent id from list_targets. Nothing to roll back is reported as ok with the agent's own message."
    )]
    async fn clear_fault(
        &self,
        Parameters(ClearFaultParams { agent }): Parameters<ClearFaultParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let config = self.config.clone();
        let out: Result<Value, String> =
            tokio::task::spawn_blocking(move || clear_fault_native(&config, &agent))
                .await
                .unwrap_or_else(|e| Err(format!("tool task failed: {e}")));
        to_tool_result(out)
    }

    #[tool(
        description = "Apply a named scenario to an explicit target agent and return the stable scenario_hash for read_report. The scenario is the report crate's Scenario JSON ({\"name\", \"injection\"}) applied through the same inject chain as apply_fault — no separate scenario engine. Game scenario templates (disconnect-on-join, combat-loss retry, brain split) are NOT shipped; this tool runs exactly what the CLI supports."
    )]
    async fn run_scenario(
        &self,
        Parameters(RunScenarioParams { agent, scenario }): Parameters<RunScenarioParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let config = self.config.clone();
        let out: Result<Value, String> =
            tokio::task::spawn_blocking(move || run_scenario_native(&config, &agent, &scenario))
                .await
                .unwrap_or_else(|e| Err(format!("tool task failed: {e}")));
        to_tool_result(out)
    }

    #[tool(
        description = "Read the RunReport archived by the coordinator for (agent, scenario_hash): scenario identity, timeline events, rollback timestamp, restore evidence and replay command. Reports are readable only by the agent whose injection produced them; an unknown hash or a non-owner reads answer 'report not found'."
    )]
    async fn read_report(
        &self,
        Parameters(ReadReportParams {
            agent,
            scenario_hash,
        }): Parameters<ReadReportParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let config = self.config.clone();
        let out: Result<Value, String> = tokio::task::spawn_blocking(move || {
            read_report_native(&config, &agent, &scenario_hash)
        })
        .await
        .unwrap_or_else(|e| Err(format!("tool task failed: {e}")));
        to_tool_result(out)
    }
}

#[derive(Parser)]
#[command(name = "faultdeck-mcp")]
struct Cli {
    /// Coordinator host (hub mode; the same chain as `faultdeck hub`)
    #[arg(long, default_value = "127.0.0.1")]
    host: String,
    /// Coordinator port
    #[arg(long, default_value_t = 7001)]
    port: u16,
    /// Coordinator token, when the coordinator requires one
    #[arg(long, allow_hyphen_values = true)]
    token: Option<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    // stdio transport: stdout carries only MCP frames; diagnostics are not
    // printed (silent discipline).
    let server = FaultdeckMcp::new(cli.host, cli.port, cli.token);
    let service = server.serve(rmcp::transport::stdio()).await?;
    service.waiting().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use faultdeck_coordinator::{AgentIdentity, CoordinatorState};
    use std::sync::{Arc, Mutex};

    #[test]
    fn build_injection_maps_kinds_like_the_cli() {
        // latency without milliseconds is rejected before any wire call.
        let err = build_injection("latency", "192.0.2.0/24", None, None, None).unwrap_err();
        assert!(err.contains("milliseconds"), "{err}");
        // loss without percent likewise.
        let err = build_injection("loss", "192.0.2.0/24", None, None, None).unwrap_err();
        assert!(err.contains("percent"), "{err}");
        // disconnect: port presence decides all_traffic, exactly as the CLI.
        let disc =
            |p: Option<u16>| match build_injection("disconnect", "192.0.2.0/24", p, None, None)
                .unwrap()
                .fault
            {
                FaultSpec::Disconnect(spec) => spec.all_traffic,
                other => panic!("wrong variant: {other:?}"),
            };
        assert!(disc(None));
        assert!(!disc(Some(443)));
        // Range validation is the coordinator's job, not duplicated here.
        let inj = build_injection("latency", "192.0.2.0/24", None, Some(100), None).unwrap();
        assert!(matches!(inj.fault, FaultSpec::Latency(_)));
    }

    #[test]
    fn build_injection_rejects_unshipped_kinds_by_name() {
        // Honesty red line: blackhole / bandwidth are not implemented; no
        // parameter combination may pretend otherwise.
        for kind in ["blackhole", "bandwidth", "toxiproxy"] {
            let err = build_injection(kind, "192.0.2.0/24", None, Some(1), Some(1.0)).unwrap_err();
            assert!(err.contains("not implemented"), "{kind}: {err}");
            assert!(err.contains("latency / loss / disconnect"), "{kind}: {err}");
        }
        let err = build_injection("latency", "not-a-cidr", None, Some(1), None).unwrap_err();
        assert!(err.contains("invalid destination"), "{err}");
    }

    #[test]
    fn response_mapping_passes_errors_and_shapes_values() {
        let err = response_to_value(AgentResponse::Error {
            message: "report not found".into(),
        })
        .unwrap_err();
        assert_eq!(err, "report not found");

        let v = response_to_value(AgentResponse::Agents {
            agents: vec![AgentIdentity {
                id: "a1".into(),
                capabilities: vec!["netem".into()],
                last_seen_ms: 42,
                online: true,
            }],
            available: vec!["a1".into()],
        })
        .unwrap();
        assert_eq!(v["agents"][0]["id"], "a1");
        assert_eq!(v["agents"][0]["online"], true);
        assert_eq!(v["available"], json!(["a1"]));

        let v = response_to_value(AgentResponse::TimedOut {
            agent_id: "a1".into(),
        })
        .unwrap();
        assert_eq!(v["status"], "timed_out");
        assert!(v["note"].as_str().unwrap().contains("before re-applying"));
    }

    #[test]
    fn native_layer_against_real_coordinator() {
        // Full native chain against an in-process coordinator: registration
        // visible via list_targets, coordinator rejections verbatim.
        let state = Arc::new(Mutex::new(CoordinatorState::default()));
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let serve_state = state.clone();
        std::thread::spawn(move || {
            let _ = faultdeck_coordinator::serve_on_with_options(
                serve_state,
                listener,
                None,
                Duration::from_secs(5),
            );
        });
        let config = HubConfig {
            host: addr.ip().to_string(),
            port: addr.port(),
            token: None,
        };

        // Register over the same control wire the CLI uses.
        {
            let stream = TcpStream::connect(addr).unwrap();
            let mut writer = BufWriter::new(stream.try_clone().unwrap());
            send_frame(
                &mut writer,
                &WireMessage {
                    token: None,
                    command: AgentCommand::Register(faultdeck_coordinator::RegisterRequest {
                        identity: AgentIdentity {
                            id: "native-e2e".into(),
                            capabilities: vec!["netem".into()],
                            last_seen_ms: 0,
                            online: false,
                        },
                    }),
                },
            )
            .unwrap();
            let mut reader = BufReader::new(stream);
            let ack: AgentResponse = read_frame(&mut reader).unwrap();
            assert!(matches!(ack, AgentResponse::Registered { .. }), "{ack:?}");
        }

        let targets = list_targets_native(&config).unwrap();
        let agents = targets["agents"].as_array().unwrap();
        assert!(agents.iter().any(|a| a["id"] == "native-e2e"), "{targets}");

        // Unknown agent: the coordinator's rejection passes through verbatim.
        let req = FaultRequest {
            fault: "latency".into(),
            destination: "192.0.2.0/24".into(),
            milliseconds: Some(100),
            ..Default::default()
        };
        let err = apply_fault_native(&config, "no-such-agent", &req).unwrap_err();
        assert!(err.contains("not registered"), "{err}");

        // Missing required parameter is rejected locally, before the wire.
        let req = FaultRequest {
            fault: "latency".into(),
            destination: "192.0.2.0/24".into(),
            ..Default::default()
        };
        let err = apply_fault_native(&config, "native-e2e", &req).unwrap_err();
        assert!(err.contains("milliseconds"), "{err}");

        // Unknown report hash: verbatim not-found, no fabrication.
        let err = read_report_native(&config, "native-e2e", "deadbeef").unwrap_err();
        assert_eq!(err, "report not found");

        // run_scenario parses strictly: garbage scenario JSON is a tool error.
        let err = run_scenario_native(&config, "native-e2e", "not json").unwrap_err();
        assert!(err.contains("invalid scenario JSON"), "{err}");
    }
}
