use std::path::PathBuf;

use faultdeck_agent::firewall::Iptables;
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

    if std::env::args().any(|a| a == "--help" || a == "-h") {
        println!("faultdeck-agent [--bind ADDR] [--token TOKEN] [--interface IFACE] [--management CIDR]... [--iptables PATH]");
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
            _ => anyhow::bail!("unknown arg {arg}"),
        }
    }

    if management.is_empty() {
        eprintln!("warning: no management networks configured; agent may fault management traffic");
    }

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
    };
    serve(config, adapter, firewall)?;
    Ok(())
}
