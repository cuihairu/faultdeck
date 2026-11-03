use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};

use faultdeck_agent::firewall::Iptables;
use faultdeck_agent::{handle_connection, AgentConfig};
use faultdeck_netem::NetemAdapter;

#[test]
fn cli_remote_ping_integration() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let cfg = AgentConfig {
        bind: addr.to_string(),
        token: None,
        interface: "eth0".into(),
        management_networks: vec![],
    };
    let adapter = NetemAdapter::dry_run("eth0");
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
fn cli_replay_command_includes_all_fault_parameters() {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::process::Command;

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let cfg = faultdeck_agent::AgentConfig {
        bind: addr.to_string(),
        token: None,
        interface: "eth0".into(),
        management_networks: vec![],
    };
    let adapter = NetemAdapter::dry_run("eth0");
    let firewall = Iptables {
        dry_run: true,
        path: None,
        runner: None,
    };
    std::thread::spawn(move || {
        if let Ok((stream, _)) = listener.accept() {
            let mut applied = std::collections::HashSet::new();
            let _ = faultdeck_agent::handle_connection(
                &std::sync::Arc::new(cfg),
                &adapter,
                &firewall,
                &mut applied,
                stream,
            );
        }
    });

    let report = std::env::temp_dir().join(format!(
        "faultdeck-replay-{}-{}.json",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let output = Command::new(env!("CARGO_BIN_EXE_faultdeck"))
        .args([
            "remote",
            "inject",
            "--host",
            &addr.ip().to_string(),
            "--port",
            &addr.port().to_string(),
            "--fault",
            "latency",
            "--destination",
            "192.0.2.0/24",
            "--milliseconds",
            "250",
            "--port-spec",
            "443",
            "--scenario-name",
            "audit-replay",
            "--report-json",
            report.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "cli failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let content = std::fs::read_to_string(&report).unwrap();
    assert!(content.contains("--milliseconds 250"));
    assert!(content.contains("--port-spec 443"));
    assert!(content.contains("--scenario-name audit-replay"));
    std::fs::remove_file(&report).unwrap();
}

#[test]
fn cli_replay_command_quotes_shell_metacharacters() {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::process::Command;

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let cfg = faultdeck_agent::AgentConfig {
        bind: addr.to_string(),
        token: None,
        interface: "eth0".into(),
        management_networks: vec![],
    };
    let adapter = NetemAdapter::dry_run("eth0");
    let firewall = Iptables {
        dry_run: true,
        path: None,
        runner: None,
    };
    std::thread::spawn(move || {
        if let Ok((stream, _)) = listener.accept() {
            let mut applied = std::collections::HashSet::new();
            let _ = faultdeck_agent::handle_connection(
                &std::sync::Arc::new(cfg),
                &adapter,
                &firewall,
                &mut applied,
                stream,
            );
        }
    });

    let report = std::env::temp_dir().join(format!(
        "faultdeck-replay-q-{}-{}.json",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let output = Command::new(env!("CARGO_BIN_EXE_faultdeck"))
        .args([
            "remote",
            "inject",
            "--host",
            &addr.ip().to_string(),
            "--port",
            &addr.port().to_string(),
            "--fault",
            "latency",
            "--destination",
            "192.0.2.0/24",
            "--milliseconds",
            "100",
            "--scenario-name",
            "audit replay $(whoami)",
            "--report-json",
            report.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "cli failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let content = std::fs::read_to_string(&report).unwrap();
    assert!(content.contains("--scenario-name 'audit replay $(whoami)'"));
    std::fs::remove_file(&report).unwrap();
}

#[test]
fn cli_send_times_out_on_unresponsive_server() {
    use std::net::TcpListener;
    use std::process::Command;

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    // Server accepts the connection but never responds, to trigger a read timeout.
    let server = std::thread::spawn(move || {
        let (_stream, _) = listener.accept().unwrap();
        std::thread::sleep(std::time::Duration::from_secs(3));
    });

    let output = Command::new(env!("CARGO_BIN_EXE_faultdeck"))
        .env("FAULTDECK_TIMEOUT_SECS", "1")
        .args([
            "remote",
            "ping",
            "--host",
            &addr.ip().to_string(),
            "--port",
            &addr.port().to_string(),
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("timed out"),
        "expected timeout message, got: {stderr}"
    );
    server.join().unwrap();
}
