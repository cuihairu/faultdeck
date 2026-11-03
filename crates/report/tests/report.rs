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
