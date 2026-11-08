# FaultDeck Desktop UI — Design

Date: 2026-11-08 · Status: approved for implementation · Shell: Tauri (Rust + WebView)

## 1. Decision

**Tauri 2** desktop shell (native window + WebView), Rust backend calls the
FaultDeck coordinator directly over the existing TCP wire protocol — no extra
HTTP layer, no Node bridge. Chosen over Electron (smaller, Rust-native, no
Node) and over a pure web dashboard (no installer — the user requires
setup.exe/dmg installers in the nightly matrix).

Linux build deps confirmed present: webkit2gtk-4.1 (2.52.6), gtk+-3.0
(3.24.52), node 24 / pnpm 12.

## 2. Architecture

```
faultdeck-desktop/  (Tauri app, added to the workspace as a crate)
  src-tauri/  (Rust): tauri commands -> coordinator wire client
      └ coordinator.rs   # TcpStream client speaking WireMessage/AgentCommand
  ui/         (frontend, pnpm + Vite + vanilla TS)
      └ src/  views + a tiny wire-client (fetch -> tauri invoke)
```

The Rust side is the **only** thing that talks to the coordinator. The
frontend calls `invoke("coordinator_list")` / `invoke("coordinator_inject", …)`
/ `invoke("coordinator_rollback", …)`; those run in Rust, open a TCP stream,
send the JSON wire message, await the framed response.

## 3. Wire contract (verbatim from crates/coordinator/src/lib.rs)

Every request is one `WireMessage` line, newline-framed, UTF-8 JSON:

```json
{"token":"<optional>","command":{"cmd":"list_agents"}}
{"token":"<optional>","command":{"cmd":"inject","id":"<agent_id>","injection":{"fault":{"kind":"latency","milliseconds":100},"target":{"source":null,"destination":"10.0.0.0/24","destination_port":9000}}}}
{"token":"<optional>","command":{"cmd":"rollback","id":"<agent_id>"}}
{"token":"<optional>,"command":{"cmd":"fetch_report","id":"<agent_id>","scenario_hash":"<hash>"}}
```

Responses are `AgentResponse` tagged on `status`:
`ok | error | registered | queued | agents{agents,available} | report{report}`.

The `agents` response drives the fleet view:
`{ "agents":[{"id","capabilities","last_seen_ms","online"}], "available":[...] }`.

The `report` response carries a `RunReport`:
`{scenario_name, scenario_yaml, scenario_hash, injected_at_ms, rolled_back_at_ms, events[], restore_evidence[], external_assertions[], replay_command}`.

Fault spec shapes (serde tag `kind`): `latency{milliseconds}`,
`loss{percent}`, `disconnect{all_traffic}`. Target:
`{source?, destination?, destination_port?}` (IpNet strings).

## 4. Views

### 4.1 Fleet (default)
- Card per agent: id, online dot, capabilities chips, last_seen.
- Header: coordinator host/port (editable), token (optional), **Connect**,
  **Refresh**.
- Per-card actions: **Inject** (opens 4.2), **Rollback** (confirm; disabled
  when nothing applied).
- Applied-fault badge: agent id → scenario hash from `applied` state
  (shown when known).

### 4.2 Inject dialog
- Fault kind segmented control: Latency / Loss / Disconnect.
  - Latency: `milliseconds` (1..86,400,000).
  - Loss: `percent` (0 < p ≤ 100).
  - Disconnect: `all_traffic` toggle.
- Target: source (optional IpNet), destination (optional IpNet),
  destination_port (optional 1..65535). At least one of destination /
  destination_port required (UI validation).
- **Apply** sends `inject`; shows the agent response (ok / queued / error).

### 4.3 Report
- List of archived reports (scenario_hash → title), latest first.
- Selected report: scenario YAML (monospace), injected/rolled-back timestamps,
  event timeline, restore-evidence table (check/passed/detail), external
  assertions (pass/fail), replay command with a **Copy** button.

### 4.4 Settings (persisted in localStorage)
- Coordinator host/port, token, heartbeat timeout.

## 5. Error & edge handling
- Coordinator unreachable: inline banner "coordinator unreachable at
  host:port" + retry; never a blank screen.
- Empty fleet: empty-state panel "no agents registered — start
  `faultdeck-agent` on the target and it appears here."
- Inject validation errors render under the field; the backend `error`
  response renders in the dialog footer.
- Long-running dispatch shows a spinner; results land in the event timeline.

## 6. Acceptance
1. `cargo build -p faultdeck-desktop` green; `tauri build` produces the
   platform installer.
2. Against a live coordinator + one agent: connect → agent appears online →
   inject latency → applied badge → rollback → badge clears.
3. Report list loads a real `RunReport`; replay command copies.
4. All above exercised in the headless check (coordinator + agent on
   127.0.0.1 ephemeral ports) as part of the nightly CI smoke.
