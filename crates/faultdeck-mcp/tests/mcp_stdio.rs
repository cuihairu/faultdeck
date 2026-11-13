//! `faultdeck-mcp` stdio end-to-end: the official rmcp client really spawns
//! this crate's binary and walks initialize → list_tools → call_tool against
//! a live in-process coordinator (the same chain the CLI hub path uses).
//! Valid JSON-RPC framing is proven by the client round-trips themselves —
//! no test stubs anywhere on the path.

use rmcp::{
    model::{CallToolRequestParams, ContentBlock},
    transport::TokioChildProcess,
    ServiceExt,
};
use serde_json::{json, Value};
use std::io::{BufReader, BufWriter};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::process::Command;

async fn call_text(
    client: &rmcp::service::RunningService<rmcp::RoleClient, ()>,
    name: &str,
    args: Value,
) -> (bool, String) {
    let params = CallToolRequestParams::new(name.to_string())
        .with_arguments(args.as_object().unwrap().clone());
    let result = client.call_tool(params).await.unwrap();
    let text = match result.content.first() {
        Some(ContentBlock::Text(t)) => t.text.clone(),
        other => panic!("unexpected content block: {other:?}"),
    };
    (result.is_error == Some(true), text)
}

#[tokio::test]
async fn mcp_stdio_end_to_end_hub_tools() -> anyhow::Result<()> {
    // Real coordinator in-process (same serve loop the binary uses).
    let state = Arc::new(Mutex::new(
        faultdeck_coordinator::CoordinatorState::default(),
    ));
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    let addr = listener.local_addr()?;
    let serve_state = state.clone();
    std::thread::spawn(move || {
        let _ = faultdeck_coordinator::serve_on_with_options(
            serve_state,
            listener,
            None,
            Duration::from_secs(5),
        );
    });

    // Register one agent over the same control wire the CLI uses, so
    // list_targets has something real to enumerate.
    {
        use faultdeck_agent::transport::{read_frame, send_frame};
        let stream = std::net::TcpStream::connect(addr)?;
        let mut writer = BufWriter::new(stream.try_clone()?);
        send_frame(
            &mut writer,
            &faultdeck_coordinator::WireMessage {
                token: None,
                command: faultdeck_coordinator::AgentCommand::Register(
                    faultdeck_coordinator::RegisterRequest {
                        identity: faultdeck_coordinator::AgentIdentity {
                            id: "mcp-e2e-agent".into(),
                            capabilities: vec!["netem".into(), "firewall".into()],
                            last_seen_ms: 0,
                            online: false,
                        },
                    },
                ),
            },
        )?;
        let mut reader = BufReader::new(stream);
        let ack: faultdeck_coordinator::AgentResponse = read_frame(&mut reader)?;
        assert!(
            matches!(ack, faultdeck_coordinator::AgentResponse::Registered { .. }),
            "{ack:?}"
        );
    }

    // Spawn the real MCP binary over stdio, pointed at the coordinator.
    let bin = env!("CARGO_BIN_EXE_faultdeck-mcp");
    let mut cmd = Command::new(bin);
    cmd.arg("--host")
        .arg(addr.ip().to_string())
        .arg("--port")
        .arg(addr.port().to_string());
    let client = ().serve(TokioChildProcess::new(cmd)?).await?;

    // 1) tools/list: exactly the five shipped tools — nothing more, so no
    //    unshipped capability (blackhole / bandwidth / scenario templates)
    //    can leak a tool past the honesty red line.
    let tools = client.list_all_tools().await?;
    let names: Vec<String> = tools.iter().map(|t| t.name.to_string()).collect();
    for expected in [
        "list_targets",
        "apply_fault",
        "clear_fault",
        "run_scenario",
        "read_report",
    ] {
        assert!(names.iter().any(|n| n == expected), "tools = {names:?}");
    }
    assert_eq!(names.len(), 5, "unexpected extra tools: {names:?}");

    // 2) list_targets: the registered agent appears with its own reported
    //    capabilities (backend availability comes from the agent, never
    //    guessed at the MCP layer).
    let (is_err, text) = call_text(&client, "list_targets", json!({})).await;
    assert!(!is_err, "{text}");
    let targets: Value = serde_json::from_str(&text)?;
    let agents = targets["agents"].as_array().unwrap();
    let agent = agents
        .iter()
        .find(|a| a["id"] == "mcp-e2e-agent")
        .expect("registered agent should be listed");
    assert_eq!(
        agent["capabilities"],
        json!(["netem", "firewall"]),
        "{agent}"
    );

    // 3) honesty red line through the full stack: an unshipped kind is
    //    rejected by name, no tool ever pretends blackhole exists.
    let (is_err, text) = call_text(
        &client,
        "apply_fault",
        json!({
            "agent": "mcp-e2e-agent",
            "fault": "blackhole",
            "destination": "192.0.2.0/24",
            "milliseconds": 100
        }),
    )
    .await;
    assert!(is_err, "{text}");
    assert!(text.contains("not implemented"), "{text}");

    // 4) missing required fault parameter is rejected locally, before any
    //    wire call, with the parameter named.
    let (is_err, text) = call_text(
        &client,
        "apply_fault",
        json!({
            "agent": "mcp-e2e-agent",
            "fault": "latency",
            "destination": "192.0.2.0/24"
        }),
    )
    .await;
    assert!(is_err, "{text}");
    assert!(text.contains("milliseconds"), "{text}");

    // 5) coordinator rejection passes through verbatim as tool-level error.
    let (is_err, text) = call_text(
        &client,
        "apply_fault",
        json!({
            "agent": "no-such-agent",
            "fault": "latency",
            "destination": "192.0.2.0/24",
            "milliseconds": 100
        }),
    )
    .await;
    assert!(is_err, "{text}");
    assert!(text.contains("not registered"), "{text}");

    // 6) read_report for an unknown hash: verbatim not-found, no fabrication.
    let (is_err, text) = call_text(
        &client,
        "read_report",
        json!({ "agent": "mcp-e2e-agent", "scenario_hash": "deadbeef" }),
    )
    .await;
    assert!(is_err, "{text}");
    assert!(text.contains("report not found"), "{text}");

    // 7) run_scenario with malformed scenario JSON is a tool-level error.
    let (is_err, text) = call_text(
        &client,
        "run_scenario",
        json!({ "agent": "mcp-e2e-agent", "scenario": "not json" }),
    )
    .await;
    assert!(is_err, "{text}");
    assert!(text.contains("invalid scenario JSON"), "{text}");

    client.cancel().await?;
    Ok(())
}
