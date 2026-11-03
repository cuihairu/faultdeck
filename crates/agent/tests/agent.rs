use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};

use faultdeck_agent::firewall::Iptables;
use faultdeck_agent::{authorize, guard_management, handle_connection, AgentConfig, Request};
use faultdeck_core::{FaultSpec, Injection, LatencySpec, Target};
use faultdeck_netem::NetemAdapter;
use ipnet::IpNet;
use std::str::FromStr;

fn latency() -> Injection {
    Injection {
        fault: FaultSpec::Latency(LatencySpec { milliseconds: 120 }),
        target: Target::destination_port(IpNet::from_str("192.0.2.0/24").unwrap(), 7000),
    }
}

fn config() -> AgentConfig {
    AgentConfig {
        bind: "127.0.0.1:0".into(),
        token: None,
        interface: "eth0".into(),
        management_networks: vec![IpNet::from_str("10.0.0.0/8").unwrap()],
    }
}

fn dummy_adapter(
) -> NetemAdapter<impl Fn(&faultdeck_netem::TcCommand) -> faultdeck_netem::Result<()>> {
    NetemAdapter::with_executor("eth0", |_: &faultdeck_netem::TcCommand| Ok(()))
}

#[test]
fn auth_rejects_wrong_token() {
    let cfg = AgentConfig {
        token: Some("secret".into()),
        ..config()
    };
    assert!(authorize(&cfg, Some("wrong")).is_err());
    assert!(authorize(&cfg, Some("secret")).is_ok());
    assert!(authorize(&cfg, None).is_err());
}

#[test]
fn management_guard_blocks_overlap() {
    let cfg = config();
    let overlapping = Injection {
        fault: FaultSpec::Latency(LatencySpec { milliseconds: 10 }),
        target: Target::destination(IpNet::from_str("10.1.2.0/24").unwrap()),
    };
    assert!(guard_management(&cfg, &overlapping).is_err());
    let ok = latency();
    assert!(guard_management(&cfg, &ok).is_ok());
}

#[test]
fn roundtrip_ping_and_rollback() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let cfg = AgentConfig {
        token: None,
        bind: addr.to_string(),
        interface: "eth0".into(),
        management_networks: vec![IpNet::from_str("10.0.0.0/8").unwrap()],
    };
    let adapter = dummy_adapter();
    let firewall = Iptables {
        dry_run: true,
        path: None,
    };
    std::thread::spawn(move || {
        if let Ok((stream, _)) = listener.accept() {
            let mut applied = std::collections::HashSet::new();
            let _ = handle_connection(
                &std::sync::Arc::new(cfg),
                &adapter,
                &firewall,
                &mut applied,
                stream,
            );
        }
    });

    let mut stream = TcpStream::connect(addr).unwrap();
    stream.write_all(b"{\"cmd\":\"ping\"}\n").unwrap();
    let mut buf = String::new();
    stream.read_to_string(&mut buf).unwrap();
    assert!(buf.contains("pong"));
}

#[test]
fn roundtrip_inject_then_rollback() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let cfg = AgentConfig {
        token: None,
        bind: addr.to_string(),
        interface: "eth0".into(),
        management_networks: vec![IpNet::from_str("10.0.0.0/8").unwrap()],
    };
    let adapter = dummy_adapter();
    let firewall = Iptables {
        dry_run: true,
        path: None,
    };
    std::thread::spawn(move || {
        let mut applied = std::collections::HashSet::new();
        for _ in 0..2 {
            if let Ok((stream, _)) = listener.accept() {
                let _ = handle_connection(
                    &std::sync::Arc::new(cfg.clone()),
                    &adapter,
                    &firewall,
                    &mut applied,
                    stream,
                );
            }
        }
    });

    let injection = serde_json::to_string(&Request::Inject(latency())).unwrap();
    let mut stream = TcpStream::connect(addr).unwrap();
    stream
        .write_all(format!("{injection}\n").as_bytes())
        .unwrap();
    let mut buf = String::new();
    stream.read_to_string(&mut buf).unwrap();
    eprintln!("inject response: {buf:?}");
    assert!(buf.contains("fault applied"));

    let mut stream = TcpStream::connect(addr).unwrap();
    stream.write_all(b"{\"cmd\":\"rollback\"}\n").unwrap();
    let mut buf = String::new();
    stream.read_to_string(&mut buf).unwrap();
    eprintln!("rollback response: {buf:?}");
    assert!(buf.contains("rolled back"));
}

#[test]
fn unauthorized_rejected_without_crash() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let cfg = AgentConfig {
        token: Some("secret".into()),
        bind: addr.to_string(),
        interface: "eth0".into(),
        management_networks: vec![],
    };
    let adapter = dummy_adapter();
    let firewall = Iptables {
        dry_run: true,
        path: None,
    };
    std::thread::spawn(move || {
        if let Ok((stream, _)) = listener.accept() {
            let mut applied = std::collections::HashSet::new();
            let _ = handle_connection(
                &std::sync::Arc::new(cfg),
                &adapter,
                &firewall,
                &mut applied,
                stream,
            );
        }
    });

    let mut stream = TcpStream::connect(addr).unwrap();
    stream.write_all(b"wrong {\"cmd\":\"ping\"}\n").unwrap();
    let mut buf = String::new();
    stream.read_to_string(&mut buf).unwrap();
    assert!(buf.contains("unauthorized"));
}
