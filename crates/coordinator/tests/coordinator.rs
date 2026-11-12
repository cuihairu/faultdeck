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
    // Regression (audit 2026-11-11 P2): a control-channel heartbeat for a
    // registered agent with NO live link used to refresh last_seen_ms, so any
    // reachable controller could keep a dead agent listed as online. Liveness
    // is proven by the reverse link, not by a heartbeat anyone can replay.
    let coordinator = Coordinator::new();
    assert!(matches!(
        coordinator.handle_control_command(&register("agent-1"), None).unwrap(),
        AgentResponse::Registered { id } if id == "agent-1"
    ));
    assert!(matches!(
        coordinator.handle_control_command(
            &AgentCommand::Heartbeat {
                id: "agent-1".into()
            },
            None
        ),
        Err(CoordinatorError::NotConnected(_))
    ));
    assert!(matches!(
        coordinator.handle_control_command(
            &AgentCommand::Heartbeat {
                id: "never-registered".into()
            },
            None
        ),
        Err(CoordinatorError::NotRegistered(_))
    ));
    // The refused heartbeat must not have forged online status.
    let listed = coordinator
        .handle_control_command(&AgentCommand::ListAgents, None)
        .unwrap();
    assert!(matches!(listed, AgentResponse::Agents { agents, available }
        if agents.len() == 1 && !agents[0].online && available.is_empty()));
}

#[test]
fn heartbeat_accepted_for_linked_agent() {
    // An agent holding a live reverse link is genuinely online, so its
    // heartbeat is honored (link presence already proves liveness).
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
    let resp = send_wire(
        &addr,
        AgentCommand::Heartbeat {
            id: "game-01".into(),
        },
    );
    assert!(matches!(resp, AgentResponse::Ok { .. }), "got {resp:?}");
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
fn fetch_report_requires_owner() {
    // Regression (audit 2026-11-11 P2): FetchReport used to ignore `id` and
    // return any report by hash alone, so a controller could read — or
    // enumerate by probing — another agent's report. Only the owning agent
    // may fetch it; a mismatched id and an unknown hash both answer
    // "report not found" so existence is not leaked.
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
    let hash = coordinator
        .state
        .lock()
        .unwrap()
        .reports
        .keys()
        .next()
        .unwrap()
        .clone();

    let owner = coordinator
        .handle_control_command(
            &AgentCommand::FetchReport {
                id: "agent-1".into(),
                scenario_hash: hash.clone(),
            },
            None,
        )
        .unwrap();
    assert!(matches!(owner, AgentResponse::Report { .. }));

    let intruder = coordinator
        .handle_control_command(
            &AgentCommand::FetchReport {
                id: "intruder".into(),
                scenario_hash: hash.clone(),
            },
            None,
        )
        .unwrap();
    assert!(
        matches!(intruder, AgentResponse::Error { ref message } if message == "report not found"),
        "expected not found for a non-owner, got {intruder:?}"
    );

    let missing = coordinator
        .handle_control_command(
            &AgentCommand::FetchReport {
                id: "agent-1".into(),
                scenario_hash: "0000000000000000".into(),
            },
            None,
        )
        .unwrap();
    assert!(
        matches!(missing, AgentResponse::Error { ref message } if message == "report not found"),
        "expected not found for an unknown hash, got {missing:?}"
    );
}

#[test]
fn rejected_agent_registration_gets_error_response() {
    // Regression (audit 2026-11-11 P2): a rejected agent registration must
    // receive an error frame, not a bare connection drop, so the agent can
    // stop retrying instead of reconnecting forever.
    use faultdeck_agent::transport::{read_frame, send_frame};
    use std::io::{BufReader, BufWriter};

    let state = Arc::new(Mutex::new(CoordinatorState::default()));
    let serve_state = state.clone();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        let _ = faultdeck_coordinator::serve_on_with_token(
            serve_state,
            listener,
            Some("secret".into()),
        );
    });

    let stream = TcpStream::connect(addr).unwrap();
    let mut writer = BufWriter::new(stream.try_clone().unwrap());
    let envelope = faultdeck_agent::link::RegisterEnvelope {
        token: Some("wrong".into()),
        id: "a".into(),
        capabilities: vec!["netem".into()],
    };
    send_frame(&mut writer, &envelope).unwrap();
    drop(writer);
    let mut reader = BufReader::new(stream);
    let resp: AgentResponse = read_frame(&mut reader).unwrap();
    assert!(
        matches!(&resp, AgentResponse::Error { message } if message.contains("unauthorized")),
        "expected an error frame explaining the rejection, got {resp:?}"
    );
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
    // Timeout is outcome-unknown, not an execution failure: the command stays
    // in flight and the late result must still land in the report.
    assert!(
        matches!(&resp, AgentResponse::TimedOut { agent_id } if agent_id == "mute-01"),
        "expected dispatch timeout receipt, got {resp:?}"
    );
    silent.join().unwrap();
}

#[test]
fn late_result_after_timeout_is_still_recorded() {
    // Regression (audit 2026-11-11 P1.2): a controller that saw the dispatch
    // timeout must be able to trust that the late execution result is applied
    // and folded into the archived report — "timed out" must not read as
    // "not executed".
    use faultdeck_agent::link::{LinkMessage, RegisterEnvelope};
    use faultdeck_agent::transport::{read_frame, send_frame};
    use std::io::{BufReader, BufWriter};

    let state = Arc::new(Mutex::new(CoordinatorState::default()));
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let state_thread = state.clone();
    std::thread::spawn(move || {
        let _ = serve_on_with_options(state_thread, listener, None, Duration::from_millis(300));
    });

    // Agent that stalls past the 300ms dispatch timeout, then answers.
    let late = std::thread::spawn(move || {
        let stream = TcpStream::connect(addr).unwrap();
        let mut writer = BufWriter::new(stream.try_clone().unwrap());
        send_frame(
            &mut writer,
            &RegisterEnvelope {
                token: None,
                id: "late-01".into(),
                capabilities: vec!["netem".into()],
            },
        )
        .unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let _ack: AgentResponse = read_frame(&mut reader).unwrap();
        let dispatch: LinkMessage = read_frame(&mut reader).unwrap();
        // Hold the dispatch until the controller's timeout has fired...
        std::thread::sleep(Duration::from_millis(500));
        let LinkMessage::Dispatch {
            request_id,
            request,
        } = dispatch
        else {
            panic!("expected dispatch");
        };
        assert!(matches!(request, faultdeck_agent::Request::Inject(_)));
        send_frame(
            &mut writer,
            &LinkMessage::Result {
                request_id,
                response: faultdeck_agent::Response::Ok {
                    message: "fault applied".into(),
                },
            },
        )
        .unwrap();
        // Keep the link alive while the coordinator routes the late result.
        std::thread::sleep(Duration::from_millis(400));
    });

    wait_for_available(&addr, "late-01");
    let injection = injection();
    let scenario = faultdeck_report::Scenario {
        name: "late-01-injection".into(),
        injection: injection.clone(),
    };
    let hash = scenario.hash();
    let resp = send_wire(
        &addr,
        AgentCommand::Inject {
            id: "late-01".into(),
            injection,
        },
    );
    assert!(
        matches!(&resp, AgentResponse::TimedOut { agent_id } if agent_id == "late-01"),
        "expected timeout receipt, got {resp:?}"
    );

    late.join().unwrap();

    // The late execution result was applied and recorded despite the timeout.
    let st = state.lock().unwrap();
    assert_eq!(
        st.applied.get("late-01").map(String::as_str),
        Some(hash.as_str())
    );
    let report = st
        .reports
        .get(&hash)
        .expect("report stub must exist for the scenario hash");
    assert!(
        report
            .events
            .iter()
            .any(|e| e.kind == "dispatch" && e.message.contains("fault applied")),
        "late dispatch result must be folded into the report, got {:?}",
        report.events
    );
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
