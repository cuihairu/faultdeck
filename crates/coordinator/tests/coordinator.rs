use faultdeck_coordinator::{
    AgentCommand, AgentIdentity, AgentResponse, Coordinator, CoordinatorState,
};
use faultdeck_core::{FaultSpec, Injection, LatencySpec, Target};
use ipnet::IpNet;
use std::str::FromStr;
use std::sync::{Arc, Mutex};

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
        coordinator.handle_control_command(&reg).unwrap(),
        AgentResponse::Registered { id } if id == "agent-1"
    ));
    assert!(matches!(
        coordinator
            .handle_control_command(&AgentCommand::Heartbeat {
                id: "agent-1".into()
            })
            .unwrap(),
        AgentResponse::Ok { .. }
    ));
    let listed = coordinator
        .handle_control_command(&AgentCommand::ListAgents)
        .unwrap();
    assert!(matches!(listed, AgentResponse::Agents { agents } if agents.len() == 1));
}

#[test]
fn inject_stores_report_and_fetches() {
    let coordinator = Coordinator::new();
    let injection = Injection {
        fault: FaultSpec::Latency(LatencySpec { milliseconds: 100 }),
        target: Target::destination_port(IpNet::from_str("192.0.2.0/24").unwrap(), 7000),
    };
    let resp = coordinator
        .handle_control_command(&AgentCommand::Inject {
            id: "agent-1".into(),
            injection,
        })
        .unwrap();
    assert!(matches!(resp, AgentResponse::Ok { .. }));

    // Compute hash by building scenario identically (hash from report stored key).
    let state = coordinator.state.lock().unwrap();
    assert_eq!(state.reports.len(), 1);
    let hash = state.reports.keys().next().unwrap().clone();
    drop(state);

    let fetched = coordinator
        .handle_control_command(&AgentCommand::FetchReport {
            id: "agent-1".into(),
            scenario_hash: hash,
        })
        .unwrap();
    assert!(matches!(fetched, AgentResponse::Report { .. }));
}

#[test]
fn unknown_heartbeat_errors() {
    let coordinator = Coordinator::new();
    assert!(matches!(
        coordinator.handle_control_command(&AgentCommand::Heartbeat { id: "nope".into() }),
        Err(faultdeck_coordinator::CoordinatorError::NotRegistered(_))
    ));
}

#[test]
fn state_shares_across_arc() {
    let state = Arc::new(Mutex::new(CoordinatorState::default()));
    let c1 = Coordinator {
        state: state.clone(),
    };
    let c2 = Coordinator { state };
    let reg = AgentCommand::Register(faultdeck_coordinator::RegisterRequest {
        identity: AgentIdentity {
            id: "a".into(),
            capabilities: vec![],
            last_seen_ms: 0,
        },
    });
    c1.handle_control_command(&reg).unwrap();
    let listed = c2
        .handle_control_command(&AgentCommand::ListAgents)
        .unwrap();
    assert!(matches!(listed, AgentResponse::Agents { agents } if agents.len() == 1));
}
