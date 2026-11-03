use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use faultdeck_core::Injection;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ReportError {
    #[error("invalid scenario hash")]
    InvalidHash,
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, ReportError>;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Scenario {
    pub name: String,
    pub injection: Injection,
}

impl Scenario {
    pub fn to_yaml(&self) -> Result<String> {
        serde_json::to_string_pretty(self).map_err(ReportError::Json)
    }

    pub fn hash(&self) -> String {
        let bytes = serde_json::to_vec(self).expect("scenario serializable");
        let digest = Sha256::digest(&bytes);
        format!("{digest:x}")
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FaultEvent {
    pub timestamp_ms: u64,
    pub kind: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RestoreEvidence {
    pub timestamp_ms: u64,
    pub check: String,
    pub passed: bool,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExternalAssertion {
    pub name: String,
    pub passed: bool,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunReport {
    pub scenario_name: String,
    pub scenario_yaml: String,
    pub scenario_hash: String,
    pub injected_at_ms: u64,
    pub rolled_back_at_ms: Option<u64>,
    pub events: Vec<FaultEvent>,
    pub restore_evidence: Vec<RestoreEvidence>,
    pub external_assertions: Vec<ExternalAssertion>,
    pub replay_command: String,
}

impl RunReport {
    pub fn new(scenario: &Scenario, injected_at_ms: u64) -> Self {
        Self {
            scenario_name: scenario.name.clone(),
            scenario_yaml: scenario.to_yaml().unwrap_or_default(),
            scenario_hash: scenario.hash(),
            injected_at_ms,
            rolled_back_at_ms: None,
            events: Vec::new(),
            restore_evidence: Vec::new(),
            external_assertions: Vec::new(),
            replay_command: String::new(),
        }
    }

    pub fn to_json(&self) -> Result<String> {
        serde_json::to_string_pretty(self).map_err(ReportError::Json)
    }

    pub fn to_html(&self) -> String {
        let mut html = String::new();
        let _ = write!(
            html,
            "<!doctype html><html><head><meta charset=\"utf-8\"><title>FaultDeck Report - {}</title>\
             <style>body{{font-family:system-ui;margin:2rem}}table{{border-collapse:collapse;width:100%}}\
             td,th{{border:1px solid #ccc;padding:.4rem;text-align:left}}h2{{margin-top:2rem}}</style></head><body>\
             <h1>FaultDeck Report: {}</h1>\
             <p>Scenario: <code>{}</code></p>\
             <p>Hash: <code>{}</code></p>\
             <p>Injected at: {}</p>",
            self.scenario_name, self.scenario_name, self.scenario_name, self.scenario_hash, self.injected_at_ms
        );
        let _ = write!(
            html,
            "<h2>Timeline</h2><table><tr><th>Timestamp</th><th>Event</th><th>Detail</th></tr>"
        );
        for event in &self.events {
            let _ = write!(
                html,
                "<tr><td>{}</td><td>{}</td><td>{}</td></tr>",
                event.timestamp_ms, event.kind, event.message
            );
        }
        let _ = write!(html, "</table><h2>Restore Evidence</h2><table><tr><th>Timestamp</th><th>Check</th><th>Result</th></tr>");
        for ev in &self.restore_evidence {
            let _ = write!(
                html,
                "<tr><td>{}</td><td>{}</td><td>{}</td></tr>",
                ev.timestamp_ms, ev.check, ev.passed
            );
        }
        let _ = write!(html, "</table><h2>External Assertions</h2><table><tr><th>Name</th><th>Passed</th><th>Detail</th></tr>");
        for a in &self.external_assertions {
            let _ = write!(
                html,
                "<tr><td>{}</td><td>{}</td><td>{}</td></tr>",
                a.name, a.passed, a.detail
            );
        }
        let _ = write!(
            html,
            "</table><h2>Replay</h2><pre>{}</pre></body></html>",
            self.replay_command
        );
        html
    }
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_millis() as u64
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct HistoricalDiff {
    pub baseline_hash: String,
    pub comparison_hash: String,
    pub changed_fields: BTreeMap<String, String>,
}

pub fn compare_runs(baseline: &RunReport, comparison: &RunReport) -> HistoricalDiff {
    let mut changed = BTreeMap::new();
    if baseline.events != comparison.events {
        changed.insert(
            "events".into(),
            format!("{} vs {}", baseline.events.len(), comparison.events.len()),
        );
    }
    if baseline.restore_evidence != comparison.restore_evidence {
        changed.insert(
            "restore_evidence".into(),
            format!(
                "{} vs {}",
                baseline.restore_evidence.len(),
                comparison.restore_evidence.len()
            ),
        );
    }
    if baseline.external_assertions != comparison.external_assertions {
        changed.insert(
            "external_assertions".into(),
            format!(
                "{} vs {}",
                baseline.external_assertions.len(),
                comparison.external_assertions.len()
            ),
        );
    }
    HistoricalDiff {
        baseline_hash: baseline.scenario_hash.clone(),
        comparison_hash: comparison.scenario_hash.clone(),
        changed_fields: changed,
    }
}
