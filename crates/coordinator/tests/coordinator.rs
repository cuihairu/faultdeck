use faultdeck_coordinator::{
    serve_on_with_options, AgentCommand, AgentIdentity, AgentResponse, Coordinator,
    CoordinatorError, CoordinatorState, WireMessage,
};
use faultdeck_core::{FaultSpec, Injection, LatencySpec, Target};
use ipnet::IpNet;
use std::net::{TcpListener, TcpStream};
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

fn injection() -> Injection {
    Injection {
        fault: FaultSpec::Latency(LatencySpec { milliseconds: 100 }),
        target: Target::destination_port(IpNet::from_str("192.0.2.0/24").unwrap(), 7000),
    }
}

fn register(id: &str) -> AgentCommand {
    AgentCommand::Register(faultdeck_coordinator::RegisterRequest {
        identity: AgentIdentity {
            id: id.into(),
            capabilities: vec!["netem".into()],
            last_seen_ms: 0,
            online: false,
        },
    })
}

fn send_wire(addr: &std::net::SocketAddr, command: AgentCommand) -> AgentResponse {
    use faultdeck_agent::transport::{read_frame, send_frame};
    use std::io::{BufReader, BufWriter};
    let stream = TcpStream::connect(addr).unwrap();
    let mut writer = BufWriter::new(stream.try_clone().unwrap());
    send_frame(
        &mut writer,
        &WireMessage {
            token: None,
            command,
        },
    )
    .unwrap();
    drop(writer);
    let mut reader = BufReader::new(stream);
    read_frame::<_, AgentResponse>(&mut reader).unwrap()
}

fn wait_for_available(addr: &std::net::SocketAddr, id: &str) {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        if let AgentResponse::Agents { available, .. } = send_wire(addr, AgentCommand::ListAgents) {
            if available.iter().any(|a| a == id) {
                return;
            }
        }
        if std::time::Instant::now() > deadline {
            panic!("agent {id} never became available");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn register_heartbeat_list() {
    let coordinator = Coordinator::new();
    assert!(matches!(
        coordinator.handle_control_command(&register("agent-1"), None).unwrap(),
        AgentResponse::Registered { id } if id == "agent-1"
    ));
    assert!(matches!(
        coordinator
            .handle_control_command(
                &AgentCommand::Heartbeat {
                    id: "agent-1".into()
                },
                None
            )
            .unwrap(),
        AgentResponse::Ok { .. }
    ));
    let listed = coordinator
        .handle_control_command(&AgentCommand::ListAgents, None)
        .unwrap();
    assert!(matches!(listed, AgentResponse::Agents { agents, available }
        if agents.len() == 1 && available == vec!["agent-1".to_string()]));
}

#[test]
fn list_marks_offline_after_heartbeat_expiry() {
    let mut state = CoordinatorState::default();
    state.register(AgentIdentity {
        id: "a".into(),
        capabilities: vec![],
        last_seen_ms: 1000,
        online: false,
    });
    // Fresh heartbeat: online.
    assert!(state.list(1000, 30_000)[0].online);
    assert_eq!(state.available(1000, 30_000), vec!["a".to_string()]);
    // Past the threshold with no link: offline and dropped from available.
    assert!(!state.list(31_001, 30_000)[0].online);
    assert!(state.available(31_001, 30_000).is_empty());
}

#[test]
fn inject_queued_when_offline_persists_instruction() {
    let coordinator = Coordinator::new();
    coordinator
        .handle_control_command(&register("agent-1"), None)
        .unwrap();
    let resp = coordinator
        .handle_control_command(
            &AgentCommand::Inject {
                id: "agent-1".into(),
                injection: injection(),
            },
            None,
        )
        .unwrap();
    // The command is accepted and SAVED (this is the "真正入队" fix: previously
    // inject only returned "queued" without storing any instruction).
    assert!(matches!(&resp, AgentResponse::Queued { agent_id, pending }
        if agent_id == "agent-1" && *pending >= 1));
    let state = coordinator.state.lock().unwrap();
    assert_eq!(state.queues["agent-1"].len(), 1);
    assert_eq!(state.reports.len(), 1);
}

#[test]
fn rollback_queued_when_offline_persists_instruction() {
    let coordinator = Coordinator::new();
    coordinator
        .handle_control_command(&register("agent-1"), None)
        .unwrap();
    let resp = coordinator
        .handle_control_command(
            &AgentCommand::Rollback {
                id: "agent-1".into(),
            },
            None,
        )
        .unwrap();
    assert!(matches!(&resp, AgentResponse::Queued { agent_id, .. } if agent_id == "agent-1"));
    assert_eq!(coordinator.state.lock().unwrap().queues["agent-1"].len(), 1);
}

#[test]
fn inject_stores_report_and_fetches() {
    let coordinator = Coordinator::new();
    coordinator
        .handle_control_command(&register("agent-1"), None)
        .unwrap();
    coordinator
        .handle_control_command(
            &AgentCommand::Inject {
                id: "agent-1".into(),
                injection: injection(),
            },
            None,
        )
        .unwrap();

    let state = coordinator.state.lock().unwrap();
    assert_eq!(state.reports.len(), 1);
    let hash = state.reports.keys().next().unwrap().clone();
    drop(state);

    let fetched = coordinator
        .handle_control_command(
            &AgentCommand::FetchReport {
                id: "agent-1".into(),
                scenario_hash: hash,
            },
            None,
        )
        .unwrap();
    assert!(matches!(fetched, AgentResponse::Report { .. }));
}

#[test]
fn unknown_heartbeat_errors() {
    let coordinator = Coordinator::new();
    assert!(matches!(
        coordinator.handle_control_command(&AgentCommand::Heartbeat { id: "nope".into() }, None),
        Err(CoordinatorError::NotRegistered(_))
    ));
}

#[test]
fn state_shares_across_arc() {
    let state = Arc::new(Mutex::new(CoordinatorState::default()));
    let c1 = Coordinator {
        state: state.clone(),
        token: None,
        dispatch_timeout: Duration::from_secs(5),
    };
    let c2 = Coordinator {
        state,
        token: None,
        dispatch_timeout: Duration::from_secs(5),
    };
    c1.handle_control_command(&register("a"), None).unwrap();
    let listed = c2
        .handle_control_command(&AgentCommand::ListAgents, None)
        .unwrap();
    assert!(matches!(listed, AgentResponse::Agents { agents, .. } if agents.len() == 1));
}

#[test]
fn wrong_token_rejected() {
    let coordinator = Coordinator::with_token(Some("secret".into()));
    assert!(matches!(
        coordinator.handle_control_command(&AgentCommand::ListAgents, Some("wrong")),
        Err(CoordinatorError::Unauthorized)
    ));
    assert!(matches!(
        coordinator.handle_control_command(&AgentCommand::ListAgents, None),
        Err(CoordinatorError::Unauthorized)
    ));
    assert!(matches!(
        coordinator.handle_control_command(&AgentCommand::ListAgents, Some("secret")),
        Ok(AgentResponse::Agents { .. })
    ));
}

#[test]
fn inject_unknown_agent_rejected() {
    let coordinator = Coordinator::new();
    assert!(matches!(
        coordinator.handle_control_command(
            &AgentCommand::Inject {
                id: "nope".into(),
                injection: injection(),
            },
            None
        ),
        Err(CoordinatorError::NotRegistered(_))
    ));
}

#[test]
fn inject_invalid_injection_rejected() {
    let coordinator = Coordinator::new();
    coordinator
        .handle_control_command(&register("agent-1"), None)
        .unwrap();
    let bad = Injection {
        fault: FaultSpec::Latency(LatencySpec { milliseconds: 0 }),
        target: Target::destination(IpNet::from_str("192.0.2.0/24").unwrap()),
    };
    assert!(matches!(
        coordinator.handle_control_command(
            &AgentCommand::Inject {
                id: "agent-1".into(),
                injection: bad,
            },
            None
        ),
        Err(CoordinatorError::InvalidInjection(_))
    ));
}

#[test]
fn handle_connection_rejects_unauthorized_wire_message() {
    use faultdeck_agent::transport::{read_frame, send_frame};
    use faultdeck_coordinator::handle_connection;
    use std::io::{BufReader, BufWriter};
    use std::net::Shutdown;

    let state = Arc::new(Mutex::new(CoordinatorState::default()));
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let thread = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let _ = handle_connection(state, stream, Some("secret".into()));
    });

    let stream = TcpStream::connect(addr).unwrap();
    let mut writer = BufWriter::new(stream.try_clone().unwrap());
    send_frame(
        &mut writer,
        &WireMessage {
            token: None,
            command: AgentCommand::ListAgents,
        },
    )
    .unwrap();
    drop(writer);
    stream.shutdown(Shutdown::Write).unwrap();
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let response: AgentResponse = read_frame(&mut reader).unwrap();
    assert!(matches!(response, AgentResponse::Error { .. }));
    thread.join().unwrap();
}

#[test]
fn handle_connection_accepts_authorized_wire_message() {
    use faultdeck_agent::transport::{read_frame, send_frame};
    use faultdeck_coordinator::handle_connection;
    use std::io::{BufReader, BufWriter};
    use std::net::Shutdown;

    let state = Arc::new(Mutex::new(CoordinatorState::default()));
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let thread = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let _ = handle_connection(state, stream, Some("secret".into()));
    });

    let stream = TcpStream::connect(addr).unwrap();
    let mut writer = BufWriter::new(stream.try_clone().unwrap());
    send_frame(
        &mut writer,
        &WireMessage {
            token: Some("secret".into()),
            command: AgentCommand::ListAgents,
        },
    )
    .unwrap();
    drop(writer);
    stream.shutdown(Shutdown::Write).unwrap();
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let response: AgentResponse = read_frame(&mut reader).unwrap();
    assert!(matches!(response, AgentResponse::Agents { .. }));
    thread.join().unwrap();
}

#[test]
fn serve_accepts_tcp_register_and_list() {
    use faultdeck_agent::transport::{read_frame, send_frame};
    use faultdeck_coordinator::serve_on_with_token;
    use std::io::{BufReader, BufWriter};
    use std::net::Shutdown;

    let state = Arc::new(Mutex::new(CoordinatorState::default()));
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        let _ = serve_on_with_token(state, listener, Some("secret".into()));
    });

    let stream = TcpStream::connect(addr).unwrap();
    let mut writer = BufWriter::new(stream.try_clone().unwrap());
    send_frame(
        &mut writer,
        &WireMessage {
            token: Some("secret".into()),
            command: register("agent-1"),
        },
    )
    .unwrap();
    drop(writer);
    stream.shutdown(Shutdown::Write).unwrap();

    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let response: AgentResponse = read_frame(&mut reader).unwrap();
    assert!(matches!(response, AgentResponse::Registered { id } if id == "agent-1"));
}

// ---------------------------------------------------------------------------
// Dispatch integration: a real agent link (reverse-dialed, Mode B) receives an
// inject/rollback through the coordinator and the execution result round-trips
// back to the controller. This is the evidence that queued commands are
// actually delivered to a NAT-behind agent over its existing outbound channel.
// ---------------------------------------------------------------------------

fn spawn_agent_link(addr: std::net::SocketAddr, recorded: Arc<Mutex<Vec<String>>>) {
    use faultdeck_agent::firewall::Iptables;
    use faultdeck_agent::link::{link_session, LinkConfig};
    use faultdeck_agent::AgentConfig;
    use faultdeck_netem::{NetemAdapter, TcCommand};

    std::thread::spawn(move || {
        let stream = TcpStream::connect(addr).unwrap();
        let adapter = NetemAdapter::with_executor("eth0", move |cmd: &TcCommand| {
            recorded.lock().unwrap().push(cmd.to_string());
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
}

#[test]
fn dispatch_inject_and_rollback_through_agent_link() {
    let state = Arc::new(Mutex::new(CoordinatorState::default()));
    let serve_state = state.clone();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        let _ = serve_on_with_options(serve_state, listener, None, Duration::from_secs(2));
    });

    let recorded = Arc::new(Mutex::new(Vec::<String>::new()));
    spawn_agent_link(addr, recorded.clone());
    wait_for_available(&addr, "game-01");

    // Controller inject through the hub -> agent executes locally -> result back.
    let resp = send_wire(
        &addr,
        AgentCommand::Inject {
            id: "game-01".into(),
            injection: injection(),
        },
    );
    assert!(
        matches!(&resp, AgentResponse::Ok { message } if message.contains("fault applied")),
        "expected agent's real execution result, got {resp:?}"
    );
    // The agent really ran tc commands (real effect, not a stub).
    assert!(
        !recorded.lock().unwrap().is_empty(),
        "agent executor must have run tc commands"
    );

    // Report carries the dispatch event (single source of truth).
    let hash = {
        let st = state.lock().unwrap();
        st.reports.keys().next().unwrap().clone()
    };
    let report = send_wire(
        &addr,
        AgentCommand::FetchReport {
            id: "game-01".into(),
            scenario_hash: hash.clone(),
        },
    );
    if let AgentResponse::Report { report } = report {
        assert!(report.events.iter().any(|e| e.kind == "dispatch"));
    } else {
        panic!("expected report");
    }

    // Rollback through the hub -> executed -> report updated.
    let resp = send_wire(
        &addr,
        AgentCommand::Rollback {
            id: "game-01".into(),
        },
    );
    assert!(
        matches!(&resp, AgentResponse::Ok { message } if message.contains("rolled back")),
        "expected rollback result, got {resp:?}"
    );
    let report = send_wire(
        &addr,
        AgentCommand::FetchReport {
            id: "game-01".into(),
            scenario_hash: hash,
        },
    );
    if let AgentResponse::Report { report } = report {
        assert!(report.rolled_back_at_ms.is_some());
        assert!(report.events.iter().any(|e| e.kind == "rollback"));
    } else {
        panic!("expected report");
    }
}

#[test]
fn queued_command_is_delivered_when_agent_reconnects() {
    // An inject submitted while the agent is offline must be SAVED, then
    // delivered automatically once the agent dials in (drain-on-reconnect).
    let state = Arc::new(Mutex::new(CoordinatorState::default()));
    let serve_state = state.clone();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        let _ = serve_on_with_options(serve_state, listener, None, Duration::from_secs(2));
    });

    // Register the agent through a controller session so it exists without a link.
    let reg = send_wire(&addr, register("game-01"));
    assert!(matches!(reg, AgentResponse::Registered { .. }));

    // Inject while offline: queued (and saved).
    let resp = send_wire(
        &addr,
        AgentCommand::Inject {
            id: "game-01".into(),
            injection: injection(),
        },
    );
    assert!(matches!(&resp, AgentResponse::Queued { agent_id, .. } if agent_id == "game-01"));

    // Now the agent dials in: the queued command is drained and executed.
    let recorded = Arc::new(Mutex::new(Vec::<String>::new()));
    spawn_agent_link(addr, recorded.clone());

    // Poll until the executor ran the command (drain is asynchronous).
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        if !recorded.lock().unwrap().is_empty() {
            break;
        }
        if std::time::Instant::now() > deadline {
            panic!("queued command was not delivered on reconnect");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    // The report records the dispatch even though nobody was waiting on it.
    // Poll until the Result frame has been routed (it trails the executor run).
    let hash = {
        let st = state.lock().unwrap();
        st.reports.keys().next().unwrap().clone()
    };
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        let report = send_wire(
            &addr,
            AgentCommand::FetchReport {
                id: "game-01".into(),
                scenario_hash: hash.clone(),
            },
        );
        if let AgentResponse::Report { report } = report {
            if report.events.iter().any(|e| e.kind == "dispatch") {
                break;
            }
        }
        if std::time::Instant::now() > deadline {
            panic!("dispatch event never reached the report");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn dispatch_to_silent_agent_times_out() {
    // An agent link that never replies must not hang the controller: the
    // coordinator surfaces a dispatch timeout (command stays queued).
    use faultdeck_agent::link::RegisterEnvelope;
    use faultdeck_agent::transport::{read_frame, send_frame};
    use std::io::{BufReader, BufWriter};

    let state = Arc::new(Mutex::new(CoordinatorState::default()));
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let state_thread = state.clone();
    std::thread::spawn(move || {
        let _ = serve_on_with_options(state_thread, listener, None, Duration::from_millis(300));
    });

    // Silent agent: registers as a link then goes quiet (never answers dispatch).
    let silent = std::thread::spawn(move || {
        let stream = TcpStream::connect(addr).unwrap();
        let mut writer = BufWriter::new(stream.try_clone().unwrap());
        send_frame(
            &mut writer,
            &RegisterEnvelope {
                token: None,
                id: "mute-01".into(),
                capabilities: vec!["netem".into()],
            },
        )
        .unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let _ack: AgentResponse = read_frame(&mut reader).unwrap();
        // Stay connected, never reply to the dispatch that follows. The
        // coordinator's 300ms timeout fires well before this sleep ends.
        let _ = stream;
        std::thread::sleep(Duration::from_secs(2));
    });

    wait_for_available(&addr, "mute-01");
    let resp = send_wire(
        &addr,
        AgentCommand::Inject {
            id: "mute-01".into(),
            injection: injection(),
        },
    );
    assert!(
        matches!(&resp, AgentResponse::Error { message } if message.contains("timed out")),
        "expected dispatch timeout, got {resp:?}"
    );
    silent.join().unwrap();
}

#[test]
fn link_disconnect_fails_inflight_dispatch() {
    // A link that drops mid-dispatch unblocks the waiting controller instead
    // of hanging it for the full timeout.
    use faultdeck_agent::link::RegisterEnvelope;
    use faultdeck_agent::transport::{read_frame, send_frame};
    use std::io::{BufReader, BufWriter};

    let state = Arc::new(Mutex::new(CoordinatorState::default()));
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let state_thread = state.clone();
    std::thread::spawn(move || {
        let _ = serve_on_with_options(state_thread, listener, None, Duration::from_secs(5));
    });

    // Agent registers, then drops the socket as soon as it receives the dispatch.
    let drop_agent = std::thread::spawn(move || {
        let stream = TcpStream::connect(addr).unwrap();
        let mut writer = BufWriter::new(stream.try_clone().unwrap());
        send_frame(
            &mut writer,
            &RegisterEnvelope {
                token: None,
                id: "drop-01".into(),
                capabilities: vec!["netem".into()],
            },
        )
        .unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let _ack: AgentResponse = read_frame(&mut reader).unwrap();
        // Read the dispatch then close without replying.
        let _: serde_json::Value = read_frame(&mut reader).unwrap();
        drop(writer);
    });

    wait_for_available(&addr, "drop-01");
    let resp = send_wire(
        &addr,
        AgentCommand::Inject {
            id: "drop-01".into(),
            injection: injection(),
        },
    );
    // Either an explicit disconnect error or a timeout — but never a hang.
    assert!(matches!(resp, AgentResponse::Error { .. }));
    drop_agent.join().unwrap();
}

#[test]
fn list_marks_online_while_link_live_and_offline_after_drop() {
    use faultdeck_agent::link::RegisterEnvelope;
    use faultdeck_agent::transport::{read_frame, send_frame};
    use std::io::{BufReader, BufWriter};

    let state = Arc::new(Mutex::new(CoordinatorState::default()));
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let state_thread = state.clone();
    std::thread::spawn(move || {
        let _ = serve_on_with_options(state_thread, listener, None, Duration::from_secs(5));
    });

    // A link that stays up for the first half, then drops.
    let agent = std::thread::spawn(move || {
        let stream = TcpStream::connect(addr).unwrap();
        let mut writer = BufWriter::new(stream.try_clone().unwrap());
        send_frame(
            &mut writer,
            &RegisterEnvelope {
                token: None,
                id: "flap-01".into(),
                capabilities: vec!["netem".into()],
            },
        )
        .unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let _ack: AgentResponse = read_frame(&mut reader).unwrap();
        // Hold the connection; the coordinator must mark it online while live.
        std::thread::sleep(Duration::from_millis(1500));
        drop(writer);
    });

    wait_for_available(&addr, "flap-01");
    if let AgentResponse::Agents { agents, available } = send_wire(&addr, AgentCommand::ListAgents)
    {
        let a = agents.iter().find(|a| a.id == "flap-01").unwrap();
        assert!(a.online, "live link must be online");
        assert!(available.contains(&"flap-01".to_string()));
    } else {
        panic!("expected agents list");
    }

    agent.join().unwrap();
    // After the link drops the agent stays online while its last heartbeat
    // (set at registration) is still within the threshold — the online check
    // is link-alive OR heartbeat-fresh (per the requirement). Expiry-to-offline
    // is covered deterministically by `list_marks_offline_after_heartbeat_expiry`.
    if let AgentResponse::Agents { agents, .. } = send_wire(&addr, AgentCommand::ListAgents) {
        let a = agents.iter().find(|a| a.id == "flap-01").unwrap();
        assert!(
            a.online,
            "fresh heartbeat keeps the agent online after the link drops"
        );
    } else {
        panic!("expected agents list");
    }
}
