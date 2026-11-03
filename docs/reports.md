# Reports

Scenario reports are produced by `faultdeck-report` and written by the CLI after a successful injection. Format: JSON (canonical) and a self-contained HTML render.

## Generating a report

```bash
faultdeck remote inject … \
  --scenario-name combat-loss \
  --report-json run1.json \
  --report-html run1.html
```

- `--scenario-name` labels the scenario (defaults to `"scenario"`).
- The report is written **locally by the CLI** after the agent confirms `fault applied`.
- `run1.html` is optional; JSON is written first.

## RunReport structure

```json
{
  "scenario_name": "combat-loss",
  "scenario_yaml": "{ \"name\": … }",      // JSON serialization of the Scenario (historical field name kept)
  "scenario_hash": "<sha256 hex>",
  "injected_at_ms": 1730000000000,
  "rolled_back_at_ms": null,
  "events": [],                            // FaultEvent { timestamp_ms, kind, message }
  "restore_evidence": [],                  // RestoreEvidence { timestamp_ms, check, passed, detail }
  "external_assertions": [],               // ExternalAssertion { name, passed, detail }
  "replay_command": "faultdeck remote inject --fault loss --destination … --host … --port …"
}
```

Key points, straight from the code:

- **`scenario_hash`** is the SHA-256 of the serialized `Scenario { name, injection }` — the same scenario always yields the same hash, which is the identity used for archival and diffing.
- **`replay_command`** is a ready-to-paste CLI invocation that reproduces the injection.
- `events`, `restore_evidence`, and `external_assertions` are part of the schema today; the CLI path currently emits them empty (only the coordinator archives a report stub at inject-queue time).

## HTML render

`RunReport::to_html()` produces a standalone page with four sections: header (name, hash, injected-at), **Timeline**, **Restore Evidence**, **External Assertions** tables, and a **Replay** block with the replay command. No external assets or JS.

## History diff

`compare_runs(baseline, comparison)` returns a `HistoricalDiff`:

```json
{
  "baseline_hash": "…",
  "comparison_hash": "…",
  "changed_fields": { "events": "2 vs 0" }
}
```

It compares the three list-valued sections (`events`, `restore_evidence`, `external_assertions`) by structural equality and records `"<baseline len> vs <comparison len>"` for each section that differs. Usage today is **library-level** (`faultdeck_report::compare_runs`) — there is no CLI subcommand for diffing yet.

## Coordinator archive

When the coordinator receives an `inject` command it computes the scenario hash and stores the fresh `RunReport` in its in-memory archive (`CoordinatorState.reports`, a `BTreeMap` keyed by hash). Clients retrieve it with `fetch_report` by hash. The archive is in-memory only — it does not survive a coordinator restart, and no filesystem report archive exists yet.
