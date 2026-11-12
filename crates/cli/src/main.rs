use std::io::{Read, Write};
use std::net::TcpStream;
use std::str::FromStr;
use std::time::Duration;

use clap::{Parser, Subcommand};
use faultdeck_agent::transport::{read_frame, send_frame};
use faultdeck_coordinator::{AgentCommand, AgentResponse, WireMessage};
use faultdeck_core::{FaultSpec, Injection, LatencySpec, LossSpec, Target};
use faultdeck_report::{RunReport, Scenario};
use ipnet::IpNet;
use serde::{Deserialize, Serialize};

#[derive(Parser)]
#[command(name = "faultdeck")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Remote(Remote),
    Hub(Hub),
}

#[derive(Parser)]
struct Remote {
    #[command(subcommand)]
    sub: RemoteSub,
}

#[derive(Parser)]
struct Hub {
    #[command(subcommand)]
    sub: HubSub,
}

#[derive(Subcommand)]
enum RemoteSub {
    Ping {
        #[arg(long)]
        host: String,
        #[arg(long, default_value_t = 7000)]
        port: u16,
        #[arg(long, allow_hyphen_values = true)]
        token: Option<String>,
    },
    Inject {
        #[arg(long)]
        host: String,
        #[arg(long, default_value_t = 7000)]
        port: u16,
        #[arg(long, allow_hyphen_values = true)]
        token: Option<String>,
        #[arg(long, value_enum)]
        fault: FaultKind,
        #[arg(long)]
        destination: String,
        #[arg(long)]
        port_spec: Option<u16>,
        #[arg(long)]
        milliseconds: Option<u64>,
        #[arg(long)]
        percent: Option<f64>,
        #[arg(long)]
        scenario_name: Option<String>,
        #[arg(long)]
        report_json: Option<String>,
        #[arg(long)]
        report_html: Option<String>,
    },
    Rollback {
        #[arg(long)]
        host: String,
        #[arg(long, default_value_t = 7000)]
        port: u16,
        #[arg(long, allow_hyphen_values = true)]
        token: Option<String>,
        /// Report file written by a prior `remote inject --report-json`;
        /// the rollback timestamp and event are folded back into it.
        #[arg(long)]
        report_json: Option<String>,
        #[arg(long)]
        report_html: Option<String>,
    },
}

#[derive(Subcommand)]
enum HubSub {
    List {
        #[arg(long)]
        host: String,
        #[arg(long, default_value_t = 7001)]
        port: u16,
        #[arg(long, allow_hyphen_values = true)]
        token: Option<String>,
    },
    Inject {
        #[arg(long)]
        host: String,
        #[arg(long, default_value_t = 7001)]
        port: u16,
        #[arg(long, allow_hyphen_values = true)]
        token: Option<String>,
        #[arg(long)]
        agent: String,
        #[arg(long, value_enum)]
        fault: FaultKind,
        #[arg(long)]
        destination: String,
        #[arg(long)]
        port_spec: Option<u16>,
        #[arg(long)]
        milliseconds: Option<u64>,
        #[arg(long)]
        percent: Option<f64>,
        #[arg(long)]
        scenario_name: Option<String>,
    },
    Rollback {
        #[arg(long)]
        host: String,
        #[arg(long, default_value_t = 7001)]
        port: u16,
        #[arg(long, allow_hyphen_values = true)]
        token: Option<String>,
        #[arg(long)]
        agent: String,
    },
}

#[derive(Clone, Copy, clap::ValueEnum)]
enum FaultKind {
    Latency,
    Loss,
    Disconnect,
}

/// Build an injection from shared CLI fault flags (used by both `remote` and
/// `hub` subcommands so the hub path reuses the exact same semantics).
fn build_injection(
    fault: FaultKind,
    destination: &str,
    port_spec: Option<u16>,
    milliseconds: Option<u64>,
    percent: Option<f64>,
) -> anyhow::Result<Injection> {
    let target = match port_spec {
        Some(p) => Target::destination_port(IpNet::from_str(destination)?, p),
        None => Target::destination(IpNet::from_str(destination)?),
    };
    let fault_spec = match fault {
        FaultKind::Latency => FaultSpec::Latency(LatencySpec {
            milliseconds: milliseconds
                .ok_or_else(|| anyhow::anyhow!("--milliseconds required for latency"))?,
        }),
        FaultKind::Loss => FaultSpec::Loss(LossSpec {
            percent: percent.ok_or_else(|| anyhow::anyhow!("--percent required for loss"))?,
        }),
        FaultKind::Disconnect => FaultSpec::Disconnect(faultdeck_core::DisconnectSpec {
            all_traffic: port_spec.is_none(),
        }),
    };
    Ok(Injection {
        fault: fault_spec,
        target,
    })
}

fn hub_send(
    host: &str,
    port: u16,
    token: Option<&str>,
    command: AgentCommand,
) -> anyhow::Result<AgentResponse> {
    let stream = TcpStream::connect((host, port))?;
    let timeout_secs: u64 = std::env::var("FAULTDECK_TIMEOUT_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(5);
    stream.set_read_timeout(Some(Duration::from_secs(timeout_secs)))?;
    stream.set_write_timeout(Some(Duration::from_secs(timeout_secs)))?;
    let mut writer = std::io::BufWriter::new(stream.try_clone()?);
    send_frame(
        &mut writer,
        &WireMessage {
            token: token.map(|t| t.to_string()),
            command,
        },
    )?;
    let mut reader = std::io::BufReader::new(stream);
    read_frame::<_, AgentResponse>(&mut reader).map_err(|e| anyhow::anyhow!(e.to_string()))
}

fn print_hub_response(response: AgentResponse) -> anyhow::Result<()> {
    match response {
        AgentResponse::Ok { message } => println!("{message}"),
        AgentResponse::Error { message } => anyhow::bail!(message),
        AgentResponse::Registered { id } => println!("registered {id}"),
        AgentResponse::Queued { agent_id, pending } => {
            println!("queued for {agent_id} (pending {pending})")
        }
        AgentResponse::TimedOut { agent_id } => {
            println!(
                "dispatch to {agent_id} timed out; the command is still in flight and its \
                 result will be recorded — check the report or `hub list` before re-injecting"
            )
        }
        AgentResponse::Agents { agents, available } => {
            for a in &agents {
                println!(
                    "{} online={} capabilities={:?} last_seen_ms={}",
                    a.id, a.online, a.capabilities, a.last_seen_ms
                );
            }
            println!("available: {available:?}");
        }
        AgentResponse::Report { .. } => anyhow::bail!("unexpected report response"),
    }
    Ok(())
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
enum WireRequest {
    Ping,
    Inject(Injection),
    Rollback,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum WireResponse {
    Ok { message: String },
    Error { message: String },
}

fn send(
    host: &str,
    port: u16,
    token: Option<&str>,
    request: &WireRequest,
) -> anyhow::Result<String> {
    let mut stream = TcpStream::connect((host, port))?;
    // Bound the whole request/response so a hung agent cannot block the CLI
    // forever. Default 5s; overridable via FAULTDECK_TIMEOUT_SECS for tests.
    let timeout_secs: u64 = std::env::var("FAULTDECK_TIMEOUT_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(5);
    stream.set_read_timeout(Some(Duration::from_secs(timeout_secs)))?;
    stream.set_write_timeout(Some(Duration::from_secs(timeout_secs)))?;
    let body = serde_json::to_string(request)?;
    let mut line = format!("{body}\n");
    if let Some(tok) = token {
        line = format!("{tok} {body}\n");
    }
    stream.write_all(line.as_bytes())?;
    let mut response = String::new();
    if let Err(e) = stream.read_to_string(&mut response) {
        if e.kind() == std::io::ErrorKind::WouldBlock || e.kind() == std::io::ErrorKind::TimedOut {
            anyhow::bail!("timed out after {timeout_secs}s waiting for {host}:{port}");
        }
        return Err(e.into());
    }
    let parsed: WireResponse = serde_json::from_str(response.trim())?;
    match parsed {
        WireResponse::Ok { message } => Ok(message),
        WireResponse::Error { message } => anyhow::bail!(message),
    }
}

/// Quote a value for safe interpolation into a `sh -c` command line. Values
/// containing shell metacharacters (spaces, quotes, globs, substitutions,
/// etc.) must be single-quoted so the generated replay_command stays
/// executable verbatim.
fn shell_quote(value: &str) -> String {
    if !value.is_empty()
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-./:@%+=".contains(&b))
    {
        value.to_string()
    } else {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Remote(remote) => match remote.sub {
            RemoteSub::Ping { host, port, token } => {
                let msg = send(&host, port, token.as_deref(), &WireRequest::Ping)?;
                println!("{msg}");
            }
            RemoteSub::Inject {
                host,
                port,
                token,
                fault,
                destination,
                port_spec,
                milliseconds,
                percent,
                scenario_name,
                report_json,
                report_html,
            } => {
                let injection =
                    build_injection(fault, &destination, port_spec, milliseconds, percent)?;
                let msg = send(
                    &host,
                    port,
                    token.as_deref(),
                    &WireRequest::Inject(injection.clone()),
                )?;
                println!("{msg}");

                if let Some(report_json) = report_json {
                    let scenario = Scenario {
                        name: scenario_name.clone().unwrap_or_else(|| "scenario".into()),
                        injection: injection.clone(),
                    };
                    let mut report = RunReport::new(&scenario, faultdeck_report::now_ms());
                    let mut replay = format!(
                        "faultdeck remote inject --fault {} --destination={} --host={} --port {}",
                        match fault {
                            FaultKind::Latency => "latency",
                            FaultKind::Loss => "loss",
                            FaultKind::Disconnect => "disconnect",
                        },
                        shell_quote(&destination),
                        shell_quote(&host),
                        port,
                    );
                    if let Some(token) = &token {
                        replay.push_str(&format!(" --token={}", shell_quote(token)));
                    }
                    if let Some(ms) = milliseconds {
                        replay.push_str(&format!(" --milliseconds {ms}"));
                    }
                    if let Some(pct) = percent {
                        replay.push_str(&format!(" --percent {pct}"));
                    }
                    if let Some(port_spec) = port_spec {
                        replay.push_str(&format!(" --port-spec {port_spec}"));
                    }
                    if let Some(name) = &scenario_name {
                        replay.push_str(&format!(" --scenario-name={}", shell_quote(name)));
                    }
                    report.replay_command = replay;
                    std::fs::write(&report_json, report.to_json()?)?;
                    if let Some(html) = report_html {
                        std::fs::write(&html, report.to_html())?;
                    }
                }
            }
            RemoteSub::Rollback {
                host,
                port,
                token,
                report_json,
                report_html,
            } => {
                let msg = send(&host, port, token.as_deref(), &WireRequest::Rollback)?;
                println!("{msg}");
                if let Some(report_json) = report_json {
                    // A report written at inject time must not claim the fault
                    // is still applied after a successful rollback: fold the
                    // rollback timestamp and event back into it.
                    let mut report: RunReport = serde_json::from_str(&std::fs::read_to_string(
                        &report_json,
                    )?)
                    .map_err(|e| anyhow::anyhow!("cannot update report {report_json}: {e}"))?;
                    let now = faultdeck_report::now_ms();
                    report.rolled_back_at_ms = Some(now);
                    report.events.push(faultdeck_report::FaultEvent {
                        timestamp_ms: now,
                        kind: "rollback".into(),
                        message: msg.clone(),
                    });
                    std::fs::write(&report_json, report.to_json()?)?;
                    if let Some(html) = report_html {
                        std::fs::write(&html, report.to_html())?;
                    }
                }
            }
        },
        Command::Hub(hub) => match hub.sub {
            HubSub::List { host, port, token } => {
                let resp = hub_send(&host, port, token.as_deref(), AgentCommand::ListAgents)?;
                print_hub_response(resp)?;
            }
            HubSub::Inject {
                host,
                port,
                token,
                agent,
                fault,
                destination,
                port_spec,
                milliseconds,
                percent,
                scenario_name,
            } => {
                let injection =
                    build_injection(fault, &destination, port_spec, milliseconds, percent)?;
                let resp = hub_send(
                    &host,
                    port,
                    token.as_deref(),
                    AgentCommand::Inject {
                        id: agent,
                        injection,
                    },
                )?;
                print_hub_response(resp)?;
                let _ = scenario_name; // hub path archives reports coordinator-side
            }
            HubSub::Rollback {
                host,
                port,
                token,
                agent,
            } => {
                let resp = hub_send(
                    &host,
                    port,
                    token.as_deref(),
                    AgentCommand::Rollback { id: agent },
                )?;
                print_hub_response(resp)?;
            }
        },
    }
    Ok(())
}
