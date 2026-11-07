use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use faultdeck_agent::firewall::Iptables;
use faultdeck_agent::link::{run_link, LinkConfig};
use faultdeck_agent::{serve, AgentConfig};
use faultdeck_netem::NetemAdapter;
use ipnet::IpNet;
use std::str::FromStr;

fn main() -> anyhow::Result<()> {
    let mut bind = "127.0.0.1:7000".to_string();
    let mut token: Option<String> = None;
    let mut interface = "eth0".to_string();
    let mut management: Vec<IpNet> = Vec::new();
    let mut iptables_path: Option<PathBuf> = None;
    let mut coordinator: Option<String> = None;
    let mut agent_id: Option<String> = None;
    let mut heartbeat_ms = faultdeck_agent::link::DEFAULT_HEARTBEAT_MS;

    if std::env::args().any(|a| a == "--help" || a == "-h") {
        println!(
            "faultdeck-agent [--bind ADDR] [--token TOKEN] [--interface IFACE] [--management CIDR]... [--iptables PATH] [--coordinator HOST:PORT --agent-id ID [--heartbeat-ms MS]]"
        );
        return Ok(());
    }
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--bind" => {
                bind = args
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("--bind needs value"))?
            }
            "--token" => {
                token = Some(
                    args.next()
                        .ok_or_else(|| anyhow::anyhow!("--token needs value"))?,
                )
            }
            "--interface" => {
                interface = args
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("--interface needs value"))?
            }
            "--management" => {
                let value = args
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("--management needs value"))?;
                management.push(IpNet::from_str(&value)?);
            }
            "--iptables" => {
                iptables_path = Some(PathBuf::from(
                    args.next()
                        .ok_or_else(|| anyhow::anyhow!("--iptables needs value"))?,
                ))
            }
            "--coordinator" => {
                coordinator = Some(
                    args.next()
                        .ok_or_else(|| anyhow::anyhow!("--coordinator needs value"))?,
                )
            }
            "--agent-id" => {
                agent_id = Some(
                    args.next()
                        .ok_or_else(|| anyhow::anyhow!("--agent-id needs value"))?,
                )
            }
            "--heartbeat-ms" => {
                heartbeat_ms = args
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("--heartbeat-ms needs value"))?
                    .parse()?;
            }
            _ => anyhow::bail!("unknown arg {arg}"),
        }
    }

    if management.is_empty() {
        eprintln!("warning: no management networks configured; agent may fault management traffic");
    }
    faultdeck_agent::warn_insecure_bind(&bind, token.as_deref());

    let config = AgentConfig {
        bind,
        token,
        interface: interface.clone(),
        management_networks: management,
    };
    let adapter = NetemAdapter::new(interface);
    let firewall = Iptables {
        dry_run: false,
        path: iptables_path,
        runner: None,
    };

    if let Some(coordinator) = coordinator {
        // Mode B: reverse-dial the coordinator; the link carries commands and
        // results. Requires an agent id for the coordinator's registry.
        let agent_id =
            agent_id.ok_or_else(|| anyhow::anyhow!("--agent-id required with --coordinator"))?;
        let link_cfg = LinkConfig {
            coordinator,
            agent_id,
            capabilities: vec!["netem".into(), "firewall".into()],
            heartbeat_ms,
            reconnect_backoff: Duration::from_secs(2),
        };
        let applied = Arc::new(Mutex::new(HashSet::new()));
        return run_link(&link_cfg, &config, &adapter, &firewall, &applied)
            .map_err(anyhow::Error::msg);
    }

    serve(config, adapter, firewall)?;
    Ok(())
}
