use std::io::{Read, Write};
use std::net::TcpStream;
use std::str::FromStr;

use clap::{Parser, Subcommand};
use faultdeck_core::{FaultSpec, Injection, LatencySpec, LossSpec, Target};
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
    let body = serde_json::to_string(request)?;
    let mut line = format!("{body}\n");
    if let Some(tok) = token {
        line = format!("{tok} {body}\n");
    }
    stream.write_all(line.as_bytes())?;
    let mut response = String::new();
    stream.read_to_string(&mut response)?;
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
                    FaultKind::Disconnect => FaultSpec::Disconnect(Default::default()),
                };
                let injection = Injection {
                    fault: fault_spec,
                    target,
                };
                let msg = send(
                    &host,
                    port,
                    token.as_deref(),
                    &WireRequest::Inject(injection),
                )?;
                println!("{msg}");
            }
            RemoteSub::Rollback { host, port, token } => {
                let msg = send(&host, port, token.as_deref(), &WireRequest::Rollback)?;
                println!("{msg}");
            }
        },
    }
    Ok(())
}
