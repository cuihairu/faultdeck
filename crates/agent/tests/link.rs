use std::collections::HashSet;
use std::io::Write;
use std::net::TcpListener;
use std::sync::Mutex;
use std::time::Duration;

use faultdeck_agent::firewall::Iptables;
use faultdeck_agent::link::{run_link, LinkConfig, LinkError};
use faultdeck_agent::transport::read_frame;
use faultdeck_agent::AgentConfig;
use faultdeck_netem::NetemAdapter;

#[test]
fn run_link_exits_on_rejection_instead_of_storm() {
    // Regression (audit 2026-11-11 P2): a rejected registration (bad token)
    // used to drop the connection with no error frame, so the agent could not
    // tell a permanent rejection from a transient drop and reconnected
    // forever. run_link must fail fast with RegisterRejected instead of
    // storming the coordinator.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { break };
            let mut reader = std::io::BufReader::new(stream.try_clone().unwrap());
            let _ = read_frame::<_, serde_json::Value>(&mut reader);
            let mut writer = stream;
            let _ = writer.write_all(
                b"{\"status\":\"error\",\"message\":\"unauthorized: missing or invalid token\"}\n",
            );
        }
    });

    let cfg = LinkConfig {
        coordinator: addr.to_string(),
        agent_id: "a".into(),
        capabilities: vec!["netem".into()],
        heartbeat_ms: 200,
        reconnect_backoff: Duration::from_millis(10),
    };
    let agent_cfg = AgentConfig {
        bind: "127.0.0.1:0".into(),
        token: Some("wrong".into()),
        interface: "eth0".into(),
        management_networks: vec![],
    };
    let adapter = NetemAdapter::dry_run("eth0");
    let firewall = Iptables {
        dry_run: true,
        path: None,
        runner: None,
    };
    let applied = Mutex::new(HashSet::new());
    let err = run_link(&cfg, &agent_cfg, &adapter, &firewall, &applied).unwrap_err();
    assert!(
        matches!(&err, LinkError::RegisterRejected { reason } if reason.contains("unauthorized")),
        "expected RegisterRejected with the coordinator's reason, got {err:?}"
    );
}
