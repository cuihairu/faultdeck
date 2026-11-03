use std::io::{Read, Write};
use std::net::TcpStream;
use std::str::FromStr;
use std::time::Duration;

use clap::{Parser, Subcommand};
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
}

#[derive(Parser)]
struct Remote {
    #[command(subcommand)]
    sub: RemoteSub,
}

#[derive(Subcommand)]
enum RemoteSub {
    Ping {
        #[arg(long)]
        host: String,
        #[arg(long, default_value_t = 7000)]
        port: u16,
        #[arg(long)]
        token: Option<String>,
    },
    Inject {
        #[arg(long)]
        host: String,
        #[arg(long, default_value_t = 7000)]
        port: u16,
        #[arg(long)]
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
        #[arg(long)]
        token: Option<String>,
    },
}

#[derive(Clone, Copy, clap::ValueEnum)]
enum FaultKind {
    Latency,
    Loss,
    Disconnect,
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
                let target = match port_spec {
                    Some(p) => Target::destination_port(IpNet::from_str(&destination)?, p),
                    None => Target::destination(IpNet::from_str(&destination)?),
                };
                let fault_spec = match fault {
                    FaultKind::Latency => FaultSpec::Latency(LatencySpec {
                        milliseconds: milliseconds.ok_or_else(|| {
                            anyhow::anyhow!("--milliseconds required for latency")
                        })?,
                    }),
                    FaultKind::Loss => FaultSpec::Loss(LossSpec {
                        percent: percent
                            .ok_or_else(|| anyhow::anyhow!("--percent required for loss"))?,
                    }),
                    FaultKind::Disconnect => {
                        FaultSpec::Disconnect(faultdeck_core::DisconnectSpec {
                            all_traffic: port_spec.is_none(),
                        })
                    }
                };
                let injection = Injection {
                    fault: fault_spec,
                    target,
                };
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
                        "faultdeck remote inject --fault {} --destination {} --host {} --port {}",
                        match fault {
                            FaultKind::Latency => "latency",
                            FaultKind::Loss => "loss",
                            FaultKind::Disconnect => "disconnect",
                        },
                        destination,
                        host,
                        port,
                    );
                    if let Some(token) = &token {
                        replay.push_str(&format!(" --token {token}"));
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
                        replay.push_str(&format!(" --scenario-name {name}"));
                    }
                    report.replay_command = replay;
                    std::fs::write(&report_json, report.to_json()?)?;
                    if let Some(html) = report_html {
                        std::fs::write(&html, report.to_html())?;
                    }
                }
            }
            RemoteSub::Rollback { host, port, token } => {
                let msg = send(&host, port, token.as_deref(), &WireRequest::Rollback)?;
                println!("{msg}");
            }
        },
    }
    Ok(())
}
