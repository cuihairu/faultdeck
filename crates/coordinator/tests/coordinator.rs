use faultdeck_coordinator::{
    AgentCommand, AgentIdentity, AgentResponse, Coordinator, CoordinatorError, CoordinatorState,
};
use faultdeck_core::{FaultSpec, Injection, LatencySpec, Target};
use ipnet::IpNet;
use std::str::FromStr;
use std::sync::{Arc, Mutex};

fn injection() -> Injection {
    Injection {
        fault: FaultSpec::Latency(LatencySpec { milliseconds: 100 }),
        target: Target::destination_port(IpNet::from_str("192.0.2.0/24").unwrap(), 7000),
    }
}

#[test]
fn register_heartbeat_list() {
    let coordinator = Coordinator::new();
    let reg = AgentCommand::Register(faultdeck_coordinator::RegisterRequest {
        identity: AgentIdentity {
            id: "agent-1".into(),
            capabilities: vec!["netem".into()],
            last_seen_ms: 0,
        },
    });
    assert!(matches!(
        coordinator.handle_control_command(&reg, None).unwrap(),
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
    assert!(matches!(listed, AgentResponse::Agents { agents } if agents.len() == 1));
}

#[test]
fn inject_stores_report_and_fetches() {
    let coordinator = Coordinator::new();
    coordinator
        .handle_control_command(
            &AgentCommand::Register(faultdeck_coordinator::RegisterRequest {
                identity: AgentIdentity {
                    id: "agent-1".into(),
                    capabilities: vec!["netem".into()],
                    last_seen_ms: 0,
                },
            }),
            None,
        )
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
    assert!(matches!(resp, AgentResponse::Ok { .. }));

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
    };
    let c2 = Coordinator { state, token: None };
    let reg = AgentCommand::Register(faultdeck_coordinator::RegisterRequest {
        identity: AgentIdentity {
            id: "a".into(),
            capabilities: vec![],
            last_seen_ms: 0,
        },
    });
    c1.handle_control_command(&reg, None).unwrap();
    let listed = c2
        .handle_control_command(&AgentCommand::ListAgents, None)
        .unwrap();
    assert!(matches!(listed, AgentResponse::Agents { agents } if agents.len() == 1));
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
    assert!(coordinator
        .handle_control_command(&AgentCommand::ListAgents, Some("secret"))
        .is_ok());
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
        .handle_control_command(
            &AgentCommand::Register(faultdeck_coordinator::RegisterRequest {
                identity: AgentIdentity {
                    id: "agent-1".into(),
                    capabilities: vec!["netem".into()],
                    last_seen_ms: 0,
                },
            }),
            None,
        )
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
    use faultdeck_coordinator::{handle_connection, WireMessage};
    use std::io::{BufReader, BufWriter};
    use std::net::Shutdown;
    use std::net::{TcpListener, TcpStream};

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
    use faultdeck_coordinator::{handle_connection, WireMessage};
    use std::io::{BufReader, BufWriter};
    use std::net::Shutdown;
    use std::net::{TcpListener, TcpStream};

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
    use faultdeck_coordinator::{serve_on_with_token, WireMessage};
    use std::io::{BufReader, BufWriter};
    use std::net::{Shutdown, TcpListener, TcpStream};

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
            command: AgentCommand::Register(faultdeck_coordinator::RegisterRequest {
                identity: AgentIdentity {
                    id: "agent-1".into(),
                    capabilities: vec!["netem".into()],
                    last_seen_ms: 0,
                },
            }),
        },
    )
    .unwrap();
    drop(writer);
    stream.shutdown(Shutdown::Write).unwrap();

    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let response: AgentResponse = read_frame(&mut reader).unwrap();
    assert!(matches!(response, AgentResponse::Registered { id } if id == "agent-1"));
}
