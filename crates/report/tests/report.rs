use faultdeck_core::{FaultSpec, Injection, LatencySpec, Target};
use faultdeck_report::{
    compare_runs, now_ms, ExternalAssertion, FaultEvent, RestoreEvidence, RunReport, Scenario,
};
use ipnet::IpNet;
use std::str::FromStr;

fn scenario() -> Scenario {
    Scenario {
        name: "latency-120ms".into(),
        injection: Injection {
            fault: FaultSpec::Latency(LatencySpec { milliseconds: 120 }),
            target: Target::destination_port(IpNet::from_str("192.0.2.0/24").unwrap(), 7000),
        },
    }
}

#[test]
fn report_json_and_html() {
    let scenario = scenario();
    let mut report = RunReport::new(&scenario, now_ms());
    report.events.push(FaultEvent {
        timestamp_ms: 1,
        kind: "inject".into(),
        message: "applied".into(),
    });
    report.restore_evidence.push(RestoreEvidence {
        timestamp_ms: 2,
        check: "ping".into(),
        passed: true,
        detail: "rtt ok".into(),
    });
    report.external_assertions.push(ExternalAssertion {
        name: "connectivity".into(),
        passed: true,
        detail: "ok".into(),
    });
    report.replay_command =
        "faultdeck remote inject --fault latency --destination 192.0.2.0/24 --milliseconds 120"
            .into();

    let json = report.to_json().unwrap();
    assert!(json.contains("scenario_hash"));
    let html = report.to_html();
    assert!(html.contains("Timeline"));
    assert!(html.contains("Replay"));
    assert!(report.scenario_hash.len() == 64);
}

#[test]
fn scenario_hash_stable() {
    let a = scenario();
    let b = scenario();
    assert_eq!(a.hash(), b.hash());
    let mut c = scenario();
    c.name = "other".into();
    assert_ne!(a.hash(), c.hash());
}

#[test]
fn scenario_yaml_is_yaml_not_json() {
    // Regression (audit 2026-11-11 P2): to_yaml emitted JSON via
    // serde_json, so the scenario_yaml field promised YAML but carried JSON
    // (braces and quoted keys). It must now render idiomatic block YAML.
    let scenario = scenario();
    let yaml = scenario.to_yaml().unwrap();
    assert!(yaml.contains("name: latency-120ms"), "got: {yaml}");
    assert!(
        !yaml.contains('{') && !yaml.contains('}'),
        "yaml must be block style, not JSON-flavored: {yaml}"
    );
    // The report field carries the same YAML content.
    let report = RunReport::new(&scenario, now_ms());
    assert!(report.scenario_yaml.contains("name: latency-120ms"));
}

#[test]
fn historical_diff() {
    let scenario = scenario();
    let mut base = RunReport::new(&scenario, 1);
    base.events.push(FaultEvent {
        timestamp_ms: 1,
        kind: "inject".into(),
        message: "applied".into(),
    });
    let mut comp = RunReport::new(&scenario, 2);
    comp.events.push(FaultEvent {
        timestamp_ms: 2,
        kind: "inject".into(),
        message: "applied".into(),
    });
    let diff = compare_runs(&base, &comp);
    assert!(diff.changed_fields.contains_key("events"));
}

#[test]
fn to_html_escapes_malicious_input() {
    let scenario = scenario();
    let mut report = RunReport::new(&scenario, now_ms());
    report.scenario_name = "<script>alert(1)</script>".into();
    report.replay_command = "echo \"&amp; <b>\"".into();
    report.events.push(FaultEvent {
        timestamp_ms: 1,
        kind: "<img src=x onerror=alert(2)>".into(),
        message: "</td><script>alert(3)</script>".into(),
    });
    report.restore_evidence.push(RestoreEvidence {
        timestamp_ms: 2,
        check: "ping '&&' ".into(),
        passed: true,
        detail: "<svg onload=alert(4)>".into(),
    });
    report.external_assertions.push(ExternalAssertion {
        name: "\"quoted\"".into(),
        passed: true,
        detail: "<script>alert(5)</script>".into(),
    });
    let html = report.to_html();
    assert!(!html.contains("<script>alert(1)"));
    assert!(!html.contains("<script>alert(3)"));
    assert!(!html.contains("<script>alert(5)"));
    assert!(!html.contains("<svg"));
    assert!(!html.contains("<img src=x"));
    assert!(!html.contains("\"quoted\""));
    assert!(html.contains("&lt;script&gt;alert(1)"));
    assert!(html.contains("&amp;amp;"));
}
