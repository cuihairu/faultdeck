use std::net::{TcpListener, TcpStream};
use std::process::Command;
use std::time::Duration;

use faultdeck_agent::firewall::Iptables;
use faultdeck_agent::link::{link_session, LinkConfig};
use faultdeck_agent::AgentConfig;
use faultdeck_coordinator::{serve_on_with_options, CoordinatorState};
use faultdeck_netem::{NetemAdapter, TcCommand};
use std::sync::{Arc, Mutex};

/// End-to-end hub path through the real binary: the agent reverse-dials the
/// coordinator (Mode B), then `faultdeck hub inject` / `hub rollback` /
/// `hub list` run against the coordinator and the agent's execution results
/// come back through the same connection.
#[test]
fn hub_cli_dispatches_to_linked_agent() {
    let state = Arc::new(Mutex::new(CoordinatorState::default()));
    let serve_state = state.clone();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        let _ = serve_on_with_options(serve_state, listener, None, Duration::from_secs(5));
    });

    let recorded = Arc::new(Mutex::new(Vec::<String>::new()));
    let rec = recorded.clone();
    let agent = std::thread::spawn(move || {
        let stream = TcpStream::connect(addr).unwrap();
        let adapter = NetemAdapter::with_executor("eth0", move |cmd: &TcCommand| {
            rec.lock().unwrap().push(cmd.to_string());
            Ok(())
        });
        let firewall = Iptables {
            dry_run: true,
            path: None,
            runner: None,
        };
        let cfg = LinkConfig {
            coordinator: addr.to_string(),
            agent_id: "game-01".into(),
            capabilities: vec!["netem".into(), "firewall".into()],
            heartbeat_ms: 200,
            reconnect_backoff: Duration::from_millis(50),
        };
        let agent_cfg = AgentConfig {
            bind: "127.0.0.1:0".into(),
            token: None,
            interface: "eth0".into(),
            management_networks: vec![],
        };
        let applied = std::sync::Mutex::new(std::collections::HashSet::new());
        let _ = link_session(&cfg, &agent_cfg, &adapter, &firewall, &applied, stream);
    });

    // Wait for the agent link to register.
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        let output = Command::new(env!("CARGO_BIN_EXE_faultdeck"))
            .args([
                "hub",
                "list",
                "--host",
                &addr.ip().to_string(),
                "--port",
                &addr.port().to_string(),
            ])
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        if stdout.contains("game-01") && stdout.contains("online=true") {
            break;
        }
        if std::time::Instant::now() > deadline {
            panic!("agent never listed online: {stdout}");
        }
        std::thread::sleep(Duration::from_millis(20));
    }

    // Inject through the hub; the agent's real execution result comes back.
    let output = Command::new(env!("CARGO_BIN_EXE_faultdeck"))
        .args([
            "hub",
            "inject",
            "--host",
            &addr.ip().to_string(),
            "--port",
            &addr.port().to_string(),
            "--agent",
            "game-01",
            "--fault",
            "latency",
            "--destination",
            "192.0.2.0/24",
            "--milliseconds",
            "250",
        ])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("fault applied"),
        "expected agent execution result via hub, got: {stdout}"
    );
    assert!(
        !recorded.lock().unwrap().is_empty(),
        "agent must have run real tc commands"
    );

    // Rollback through the hub.
    let output = Command::new(env!("CARGO_BIN_EXE_faultdeck"))
        .args([
            "hub",
            "rollback",
            "--host",
            &addr.ip().to_string(),
            "--port",
            &addr.port().to_string(),
            "--agent",
            "game-01",
        ])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("rolled back"),
        "expected rollback result via hub, got: {stdout}"
    );
    drop(agent);
}
