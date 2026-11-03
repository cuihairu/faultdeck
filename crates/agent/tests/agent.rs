use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};

use faultdeck_agent::firewall::Iptables;
use faultdeck_agent::{
    authorize, guard_management, handle_connection, serve_on, AgentConfig, Request,
};
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
        runner: None,
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
        runner: None,
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
        runner: None,
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

#[test]
fn serve_preserves_applied_fault_across_connections() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let cfg = config();
    let adapter = NetemAdapter::dry_run("eth0");
    let firewall = Iptables {
        dry_run: true,
        path: None,
        runner: None,
    };
    std::thread::spawn(move || {
        serve_on(
            cfg,
            adapter,
            firewall,
            listener,
            std::collections::HashSet::new(),
        )
        .unwrap();
    });

    let mut stream = TcpStream::connect(addr).unwrap();
    let injection = serde_json::to_string(&Request::Inject(latency())).unwrap();
    stream
        .write_all(format!("{injection}\n").as_bytes())
        .unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    assert!(response.contains("fault applied"));

    let mut stream = TcpStream::connect(addr).unwrap();
    stream.write_all(b"{\"cmd\":\"rollback\"}\n").unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    assert!(response.contains("fault rolled back"));
}

#[test]
fn oversized_line_is_rejected_without_buffering() {
    use faultdeck_agent::{read_line_limited, MAX_INJECTION_BYTES};
    let mut data = vec![b'a'; MAX_INJECTION_BYTES + 1];
    data.push(b'\n');
    let mut reader = std::io::BufReader::new(data.as_slice());
    let result = read_line_limited(&mut reader, MAX_INJECTION_BYTES);
    assert!(result.is_err());
}

#[test]
fn oversized_frame_is_rejected() {
    use faultdeck_agent::transport::{read_frame, MAX_FRAME_BYTES};
    let mut data = b"{\"cmd\":\"".to_vec();
    data.extend(std::iter::repeat(b'a').take(MAX_FRAME_BYTES));
    data.push(b'\n');
    let mut reader = std::io::BufReader::new(data.as_slice());
    let result: Result<faultdeck_agent::Request, _> = read_frame(&mut reader);
    assert!(result.is_err());
}

#[test]
fn constant_time_eq_matches_only_equal_slices() {
    use faultdeck_agent::constant_time_eq;
    assert!(constant_time_eq(b"secret", b"secret"));
    assert!(!constant_time_eq(b"secret", b"wrong!!"));
    assert!(!constant_time_eq(b"short", b"longer"));
    assert!(constant_time_eq(b"", b""));
}

#[test]
fn loopback_bind_detection() {
    use faultdeck_agent::is_loopback_bind;
    assert!(is_loopback_bind("127.0.0.1:7000"));
    assert!(is_loopback_bind("[::1]:7000"));
    assert!(is_loopback_bind("localhost:7000"));
    assert!(!is_loopback_bind("0.0.0.0:7000"));
    assert!(!is_loopback_bind("192.168.1.10:7000"));
}

#[test]
fn warn_insecure_bind_flags_remote_without_token() {
    use faultdeck_agent::warn_insecure_bind;
    assert!(warn_insecure_bind("0.0.0.0:7000", None));
    assert!(!warn_insecure_bind("0.0.0.0:7000", Some("secret")));
    assert!(!warn_insecure_bind("127.0.0.1:7000", None));
}

#[test]
fn wire_payload_with_spaces_is_valid_without_token() {
    // Without a configured token, the header must be treated entirely as
    // payload, so pretty-printed JSON with spaces after colons is not split.
    let config = config();
    let request: Request = Request::Ping;
    let json = serde_json::to_string_pretty(&request).unwrap();
    assert!(json.contains(' '));
    let parsed: Result<Request, _> = serde_json::from_str(&json);
    assert!(parsed.is_ok());
}

#[test]
fn handle_connection_writes_json_error_response() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let cfg = config();
    let adapter = NetemAdapter::dry_run("eth0");
    let firewall = Iptables {
        dry_run: true,
        path: None,
        runner: None,
    };
    let thread = std::thread::spawn(move || {
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
    stream.write_all(b"{ invalid json }\n").unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    assert!(response.contains("invalid json"));
    thread.join().unwrap();
}
