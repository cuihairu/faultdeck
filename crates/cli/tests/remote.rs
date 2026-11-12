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
            let applied = std::sync::Mutex::new(std::collections::HashSet::new());
            let _ = handle_connection(
                &std::sync::Arc::new(cfg),
                &adapter,
                &firewall,
                &applied,
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
            let applied = std::sync::Mutex::new(std::collections::HashSet::new());
            let _ = faultdeck_agent::handle_connection(
                &std::sync::Arc::new(cfg),
                &adapter,
                &firewall,
                &applied,
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
    assert!(content.contains("--scenario-name=audit-replay"));
    std::fs::remove_file(&report).unwrap();
}

#[test]
fn cli_replay_command_quotes_shell_metacharacters() {
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
    // Accept two connections: one for the initial CLI inject, a second for
    // the replay command we later execute under `sh -c`.
    std::thread::spawn(move || {
        for _ in 0..2 {
            if let Ok((stream, _)) = listener.accept() {
                let applied = std::sync::Mutex::new(std::collections::HashSet::new());
                let _ = faultdeck_agent::handle_connection(
                    &std::sync::Arc::new(cfg.clone()),
                    &adapter,
                    &firewall,
                    &applied,
                    stream,
                );
            }
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
    let report_data: serde_json::Value = serde_json::from_str(&content).unwrap();
    let replay = report_data["replay_command"].as_str().unwrap();
    assert!(replay.contains("--scenario-name='audit replay $(whoami)'"));
    // Execute the replay command under sh -c to prove the quoting really is
    // shell-safe, not merely aesthetically wrapped in single quotes.
    let bin = env!("CARGO_BIN_EXE_faultdeck");
    let dir = std::path::Path::new(bin).parent().unwrap();
    let cmd = format!("PATH={}:$PATH; {}", dir.display(), replay);
    let status = Command::new("sh").arg("-c").arg(cmd).status().unwrap();
    assert!(status.success(), "replay failed: {replay}");

    std::fs::remove_file(&report).unwrap();
}

#[test]
fn cli_replay_command_quotes_embedded_single_quotes() {
    use std::net::TcpListener;
    use std::process::Command;

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
        for _ in 0..2 {
            if let Ok((stream, _)) = listener.accept() {
                let applied = std::sync::Mutex::new(std::collections::HashSet::new());
                let _ = faultdeck_agent::handle_connection(
                    &std::sync::Arc::new(cfg.clone()),
                    &adapter,
                    &firewall,
                    &applied,
                    stream,
                );
            }
        }
    });

    let report = std::env::temp_dir().join(format!(
        "faultdeck-replay-quote-{}-{}.json",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let scenario_name = r#"audit 'replay' $(id)"#;
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
            scenario_name,
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
    let report_data: serde_json::Value = serde_json::from_str(&content).unwrap();
    let replay = report_data["replay_command"].as_str().unwrap();
    assert!(replay.contains(r#"--scenario-name='audit '\''replay'\'' $(id)'"#));
    let bin = env!("CARGO_BIN_EXE_faultdeck");
    let dir = std::path::Path::new(bin).parent().unwrap();
    let cmd = format!("PATH={}:$PATH; {}", dir.display(), replay);
    let status = Command::new("sh").arg("-c").arg(cmd).status().unwrap();
    assert!(status.success(), "replay failed: {replay}");
    std::fs::remove_file(&report).unwrap();
}

#[test]
fn cli_replay_command_handles_dash_leading_token() {
    use std::net::TcpListener;
    use std::process::Command;

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let cfg = AgentConfig {
        bind: addr.to_string(),
        token: Some("-leading-dash".into()),
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
        for _ in 0..2 {
            if let Ok((stream, _)) = listener.accept() {
                let applied = std::sync::Mutex::new(std::collections::HashSet::new());
                let _ = faultdeck_agent::handle_connection(
                    &std::sync::Arc::new(cfg.clone()),
                    &adapter,
                    &firewall,
                    &applied,
                    stream,
                );
            }
        }
    });

    let report = std::env::temp_dir().join(format!(
        "faultdeck-replay-dash-{}-{}.json",
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
            "--token",
            "-leading-dash",
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
    let report_data: serde_json::Value = serde_json::from_str(&content).unwrap();
    let replay = report_data["replay_command"].as_str().unwrap();
    // The replay must carry the token as --token=<value> so a leading dash is
    // parsed as the option's value, not as a new flag by clap.
    assert!(replay.contains("--token=-leading-dash"));
    let bin = env!("CARGO_BIN_EXE_faultdeck");
    let dir = std::path::Path::new(bin).parent().unwrap();
    let cmd = format!("PATH={}:$PATH; {}", dir.display(), replay);
    let status = Command::new("sh").arg("-c").arg(cmd).status().unwrap();
    assert!(status.success(), "replay failed: {replay}");
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

#[test]
fn cli_rollback_updates_the_inject_report() {
    // Regression (audit 2026-11-11 P1.3): a report written at inject time
    // used to keep rolled_back_at_ms unset forever, so rollback — an MVP
    // acceptance gate — was never reflected in the report data. Passing the
    // same --report-json to `remote rollback` must fold the rollback
    // timestamp and event back into the file.
    use std::net::TcpListener;
    use std::process::Command;

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
    // Two connections: inject, then rollback. One shared applied state so the
    // rollback actually finds the fault the inject left behind.
    let applied = std::sync::Mutex::new(std::collections::HashSet::new());
    std::thread::spawn(move || {
        for _ in 0..2 {
            if let Ok((stream, _)) = listener.accept() {
                let _ = faultdeck_agent::handle_connection(
                    &std::sync::Arc::new(cfg.clone()),
                    &adapter,
                    &firewall,
                    &applied,
                    stream,
                );
            }
        }
    });

    let report = std::env::temp_dir().join(format!(
        "faultdeck-rollback-report-{}-{}.json",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let html = std::env::temp_dir().join(format!(
        "faultdeck-rollback-report-{}-{}.html",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let common = [
        "--host",
        &addr.ip().to_string(),
        "--port",
        &addr.port().to_string(),
    ];

    let output = Command::new(env!("CARGO_BIN_EXE_faultdeck"))
        .args([
            "remote",
            "inject",
            "--fault",
            "latency",
            "--destination",
            "192.0.2.0/24",
            "--milliseconds",
            "100",
            "--report-json",
            report.to_str().unwrap(),
        ])
        .args(common)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "inject failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Before rollback the report must not claim one happened.
    let content = std::fs::read_to_string(&report).unwrap();
    let before: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert!(before["rolled_back_at_ms"].is_null());

    let output = Command::new(env!("CARGO_BIN_EXE_faultdeck"))
        .args([
            "remote",
            "rollback",
            "--report-json",
            report.to_str().unwrap(),
            "--report-html",
            html.to_str().unwrap(),
        ])
        .args(common)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "rollback failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let content = std::fs::read_to_string(&report).unwrap();
    let after: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert!(
        !after["rolled_back_at_ms"].is_null(),
        "rolled_back_at_ms must be set after rollback"
    );
    let events = after["events"].as_array().unwrap();
    assert!(
        events.iter().any(|e| {
            e["kind"] == "rollback"
                && e["message"]
                    .as_str()
                    .map(|m| m.contains("rolled back"))
                    .unwrap_or(false)
        }),
        "rollback event must be recorded, got {events:?}"
    );

    // The HTML twin carries the same truth.
    let html_out = std::fs::read_to_string(&html).unwrap();
    assert!(html_out.contains("Rolled back") || html_out.contains("rolled back"));

    std::fs::remove_file(&report).unwrap();
    std::fs::remove_file(&html).unwrap();
}

#[test]
fn cli_authenticates_token_with_spaces() {
    // Regression (audit 2026-11-11 P2): the remote header framed the token as
    // a space-delimited prefix, so a token containing a space was split at the
    // first space and could never authenticate. The token is percent-encoded
    // on the wire; a spaced token must now authenticate end to end.
    use std::net::TcpListener;
    use std::process::Command;

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let cfg = AgentConfig {
        bind: addr.to_string(),
        token: Some("my secret token".into()),
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
            let applied = std::sync::Mutex::new(std::collections::HashSet::new());
            let _ = faultdeck_agent::handle_connection(
                &std::sync::Arc::new(cfg),
                &adapter,
                &firewall,
                &applied,
                stream,
            );
        }
    });

    let output = Command::new(env!("CARGO_BIN_EXE_faultdeck"))
        .args([
            "remote",
            "ping",
            "--host",
            &addr.ip().to_string(),
            "--port",
            &addr.port().to_string(),
            "--token",
            "my secret token",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "spaced token must authenticate, cli failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("pong"));
}

#[test]
fn cli_rejects_wrong_token_with_spaces() {
    use std::net::TcpListener;
    use std::process::Command;

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let cfg = AgentConfig {
        bind: addr.to_string(),
        token: Some("my secret token".into()),
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
            let applied = std::sync::Mutex::new(std::collections::HashSet::new());
            let _ = faultdeck_agent::handle_connection(
                &std::sync::Arc::new(cfg),
                &adapter,
                &firewall,
                &applied,
                stream,
            );
        }
    });

    let output = Command::new(env!("CARGO_BIN_EXE_faultdeck"))
        .args([
            "remote",
            "ping",
            "--host",
            &addr.ip().to_string(),
            "--port",
            &addr.port().to_string(),
            "--token",
            "wrong secret",
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("unauthorized"),
        "expected unauthorized, got: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn cli_bails_on_oversized_response() {
    // Regression (audit 2026-11-11 P2): the response read used
    // read_to_string, buffering an unbounded peer-controlled payload before
    // any size check could run. A peer streaming more than the 64 KiB line
    // ceiling must produce a clear error, not balloon memory.
    use std::io::Write;
    use std::net::TcpListener;
    use std::process::Command;

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let junk = vec![b'a'; 128 * 1024];
        let _ = stream.write_all(&junk);
        // Give the client time to read past the ceiling before closing.
        std::thread::sleep(std::time::Duration::from_millis(300));
    });

    let output = Command::new(env!("CARGO_BIN_EXE_faultdeck"))
        .env("FAULTDECK_TIMEOUT_SECS", "2")
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
    assert!(
        !output.status.success(),
        "CLI must fail on an oversized response"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("maximum size"),
        "expected the bounded-read error, got: {stderr}"
    );
    server.join().unwrap();
}
