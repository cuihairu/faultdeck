# Architecture

FaultDeck is a Rust workspace. The current implementation ships **eight built crates** plus one empty placeholder (`faultdeck-protocol` — the directory exists but contains no `Cargo.toml` and is **not** a workspace member yet). The planned toxiproxy backend and scenario YAML DSL from the [MVP plan](/design/mvp-plan) are not implemented; today's backend is `tc netem` + `iptables`.

## Crates

| crate | binary | responsibility |
|---|---|---|
| `faultdeck-core` | — | shared model: `FaultSpec` (latency / loss / disconnect), `Target`, `Injection`, intrinsic validation (`validate_injection`), management-network overlap check |
| `faultdeck-netem` | — | `tc` adapter: plans and executes `tc qdisc`/`filter` commands, preflight checks, rollback, `dry_run` executor |
| `faultdeck-agent` | `faultdeck-agent` | resident TCP daemon: token auth, request handling, iptables firewall (disconnect), single-active-fault state |
| `faultdeck-coordinator` | `faultdeck-coordinator` | multi-target hub: agent registry (register/heartbeat/list with online status), report archive, live inject/rollback dispatch over agent reverse links, per-agent queues for offline agents |
| `faultdeck-report` | — | `Scenario`, `RunReport` (JSON/HTML), SHA-256 scenario hash, `compare_runs` history diff |
| `faultdeck-cli` | `faultdeck` | CLI entry: `faultdeck remote ping / inject / rollback` (direct to agent) and `faultdeck hub list / inject / rollback` (via coordinator) |
| `faultdeck-desktop` | `faultdeck-desktop` | Tauri desktop UI: fleet / injection / report views over `coordinator_list` · `coordinator_inject` · `coordinator_rollback` · `coordinator_report` (`crates/desktop/src/lib.rs`) |
| `faultdeck-mcp` | `faultdeck-mcp` | MCP server for AI agents (stdio, official `rmcp` SDK): `list_targets` / `apply_fault` / `clear_fault` / `run_scenario` / `read_report` over the coordinator wire protocol |
| `faultdeck-protocol` | — | *placeholder, not in workspace* |

## Interaction overview

```
 faultdeck CLI                faultdeck-agent                target host
 ┌──────────────┐   TCP,      ┌──────────────────┐   tc      ┌──────────┐
 │ remote ping  │──newline──▶ │ token auth       │──qdisc──▶ │  eth0    │
 │ remote inject│   framed    │ mgmt-network guard│──filter─▶ │ (netem)  │
 │ remote       │   JSON      │ validate_injection│           └──────────┘
 │   rollback   │             │                  │  iptables ┌──────────┐
 └──────────────┘             │ firewall (mangle)│──DROP────▶│ disconnect│
                              └──────────────────┘  -t mangle└──────────┘

 faultdeck-coordinator (default :7001)
 ┌────────────────────────────────────────────┐
 │ registry: agents / reports / links /       │◀── framed JSON lines (multi-message
 │   queues                                   │    connections, threaded accept)
 │ AgentCommand: register, heartbeat,         │◀── agent reverse links: RegisterEnvelope,
 │   inject, rollback, fetch_report,          │    then LinkMessage::Dispatch → Result /
 │   list_agents                              │    Heartbeat on the same connection
 └────────────────────────────────────────────┘
```

## Wire protocol (CLI ↔ agent)

One TCP connection per CLI invocation. The client sends a single line; the agent answers with a single JSON line and closes.

- With token: `<token> {json}\n` — without: `{json}\n`
- Request: `{"cmd":"ping"}` | `{"cmd":"inject", ...Injection}` | `{"cmd":"rollback"}`
- Response: `{"status":"ok","message":"..."}` | `{"status":"error","message":"..."}`

## Wire protocol (coordinator)

Long-lived connections, newline-framed JSON lines in both directions (`faultdeck-agent::transport`). One listener serves two session kinds, disambiguated by the first frame:

- **Controller session** — a `WireMessage` envelope per command. `AgentCommand`: `register` | `heartbeat` | `inject` | `rollback` | `fetch_report` | `list_agents`; `AgentResponse`: `ok` | `error` | `registered` | `queued` | `agents` | `report`.
- **Agent link** — the agent dials the coordinator outbound (double-NAT safe) and sends a `RegisterEnvelope` first (`crates/agent/src/link.rs:46`); afterwards the coordinator writes `LinkMessage::Dispatch` frames down the same connection and the agent answers with `LinkMessage::Result` plus periodic `LinkMessage::Heartbeat`s.

State lives in a `Mutex<CoordinatorState>`: `agents`, `reports`, `links` (live reverse connections), and per-agent `queues` are `BTreeMap`s.

**Implementation status:** `inject`/`rollback` validate, archive a report stub, and then dispatch for real — a live link receives the command immediately, the agent's execution result is returned to the waiting controller (bounded by `DEFAULT_DISPATCH_TIMEOUT`, 5 s) and folded into the archived report; if the agent is offline the command persists in its queue and is drained on its next registration. `list_agents` annotates each `AgentIdentity.online` from link liveness or heartbeat freshness (`HEARTBEAT_TIMEOUT_MS`, 30 s) and offline agents are excluded from `available`.

## Failure handling in the netem adapter

1. `plan()` validates and returns the command list without executing.
2. `inject()` runs a preflight `tc qdisc show` — refuses to touch an interface whose root qdisc is not FaultDeck's `100:` handle.
3. Any leftover FaultDeck root qdisc is cleaned up before applying.
4. If any command fails mid-apply, the adapter immediately issues the rollback command.
5. `rollback()` deletes the root qdisc; "not found"-style `tc` errors are tolerated (idempotent).

Disconnect faults bypass `tc` entirely — see [Netem Faults](/netem-faults#disconnect).

## Source layer map

The source responsibility layers are independent of crate-directory names. Terms are synchronized in the [Glossary](/glossary).

### 1. Shared semantics and safety model

`crates/core/src/lib.rs` owns `FaultSpec`, `Target`, `Injection`, intrinsic validation, and management-network overlap checks (`crates/core/src/lib.rs:26`, `crates/core/src/lib.rs:44`, `crates/core/src/lib.rs:82`, `crates/core/src/lib.rs:108`, `crates/core/src/lib.rs:142`). It is persistent-state-free and has no network I/O. Its schema is Rust Serde plus the intrinsic validation errors at `crates/core/src/lib.rs:88`; fault-facing prose is in [Netem Faults](/netem-faults), and no dedicated protocol file exists.

### 2. Policy-to-backend translation

`crates/netem/src/lib.rs` translates validated latency/loss into ordered `tc qdisc` and `filter` commands through `NetemAdapter` (`crates/netem/src/lib.rs:177`, `crates/netem/src/lib.rs:196`, `crates/netem/src/lib.rs:203`). Its execution port is `CommandExecutor` (`crates/netem/src/lib.rs:77`), the command schema is `TcCommand` (`crates/netem/src/lib.rs:61`), and runtime inputs are the interface name plus the optional dry-run executor setting (`crates/netem/src/lib.rs:177`, `crates/netem/src/lib.rs:187`). It stores no state between calls and exposes no network protocol; implementation detail is in [Architecture](/architecture#failure-handling-in-the-netem-adapter).

### 3. Host execution and guard

`crates/agent/src/lib.rs` is the agent host: it accepts TCP, authenticates, guards management CIDRs, decodes requests, routes disconnect or netem, and keeps the active-fault marker in `Arc<Mutex<HashSet<String>>>` (`crates/agent/src/lib.rs:312`, `crates/agent/src/lib.rs:417`, `crates/agent/src/lib.rs:237`). Its config schema is `AgentConfig` (`crates/agent/src/lib.rs:137`), populated by `crates/agent/src/main.rs:87`; [Configuration](/configuration) documents those inputs. The `Iptables` backend in `crates/agent/src/firewall.rs:37` executes host rules, while protocol types remain in `crates/agent/src/lib.rs:157` and `crates/agent/src/lib.rs:165`; no dedicated protocol file exists.

### 4. Wire transport and topology

`crates/agent/src/transport.rs` owns bounded NDJSON framing plus Mode A listen/accept and Mode B reverse connect (`crates/agent/src/transport.rs:27`, `crates/agent/src/transport.rs:94`, `crates/agent/src/transport.rs:118`). It is stateless except accepted sockets. The framing schema is the generic `send_frame`/`read_frame` pair (`crates/agent/src/transport.rs:57`, `crates/agent/src/transport.rs:65`). On top of that framing, `crates/agent/src/link.rs` implements the live Mode B link session — register envelope, heartbeat thread, dispatch pump, and the reconnecting `run_link` loop (`crates/agent/src/link.rs:46`, `crates/agent/src/link.rs:77`, `crates/agent/src/link.rs:148`, `crates/agent/src/link.rs:192`). Topology prose is in [Agents](/remote-agents), and payload types live in the agent and coordinator layers rather than a dedicated protocol crate (`crates/protocol/src` is empty).

### 5. User boundary and report materialization

`crates/cli/src/main.rs` converts `remote ping/inject/rollback` and `hub list/inject/rollback` arguments into shared `Injection` values, performs bounded TCP exchanges with the agent or coordinator, optionally materializes JSON/HTML reports, and generates a replay command (`crates/cli/src/main.rs:142`, `crates/cli/src/main.rs:165`, `crates/cli/src/main.rs:226`, `crates/cli/src/main.rs:341`). It owns no server state; file output is caller-directed. The command schema is the clap tree at `crates/cli/src/main.rs:22`, `FAULTDECK_TIMEOUT_SECS` is the sole environment input (`crates/cli/src/main.rs:248`), and user-facing flags are documented in [Configuration](/configuration).

### 6. Fleet registry and control-plane state

`crates/coordinator/src/lib.rs` runs a threaded TCP hub, stores agents, reports, live links, and per-agent queues in `Arc<Mutex<CoordinatorState>>`, and handles controller sessions plus agent reverse links (`crates/coordinator/src/lib.rs:176`, `crates/coordinator/src/lib.rs:270`, `crates/coordinator/src/lib.rs:466`, `crates/coordinator/src/lib.rs:566`). All state is volatile. `AgentIdentity`, `AgentCommand`, `AgentResponse`, and `WireMessage` define that schema (`crates/coordinator/src/lib.rs:49`, `crates/coordinator/src/lib.rs:67`, `crates/coordinator/src/lib.rs:92`, `crates/coordinator/src/lib.rs:270`); `crates/coordinator/src/main.rs:4` supplies bind/token inputs documented in [Configuration](/configuration). [Agents](/remote-agents) carries the operational protocol prose.

### 7. Evidence and reproducibility model

`crates/report/src/lib.rs` owns `Scenario`, scenario hashing, `RunReport`, HTML rendering, timestamps, and report comparison (`crates/report/src/lib.rs:41`, `crates/report/src/lib.rs:57`, `crates/report/src/lib.rs:87`, `crates/report/src/lib.rs:118`, `crates/report/src/lib.rs:176`, `crates/report/src/lib.rs:190`). It is persistent-state-free; CLI and coordinator decide whether to retain output. Its Serde schema is the report model itself, HTML copy is generated in the same layer, and dedicated user prose is in [Reports](/reports).

### 8. Agent-facing shell layer

`crates/faultdeck-mcp/src/main.rs` is an MCP server (stdio, official `rmcp` SDK) that lets AI agents drive the fleet. Its five tools — `list_targets`, `apply_fault`, `clear_fault`, `run_scenario`, `read_report` — speak the coordinator wire protocol (`WireMessage`/`AgentCommand`) over the same `faultdeck_agent::transport` framing the CLI hub path uses (`crates/faultdeck-mcp/src/main.rs`). The layer has no injection implementation of its own: validation stays in the coordinator, application stays in the agent, and fault-kind availability is whatever the agents report through `list_targets`.

### 9. Desktop shell layer

`crates/desktop/src/lib.rs` is the Tauri desktop UI: fleet, injection and report views, with the Rust side calling `coordinator_list`, `coordinator_inject`, `coordinator_rollback` and `coordinator_report` (`crates/desktop/src/lib.rs:162`, `crates/desktop/src/lib.rs:172`, `crates/desktop/src/lib.rs:193`, `crates/desktop/src/lib.rs:204`). The Rust commands speak the coordinator wire protocol directly, so the UI adds no second control-plane path.

### Why these layers are split

1. `core::Injection` is consumed by both direct CLI construction and coordinator validation, so semantic validation must not depend on either transport host (`crates/cli/src/main.rs:142`, `crates/coordinator/src/lib.rs:351`).
2. Netem exposes plan, inject, and rollback separately; the agent needs execution while adapters and tests need dry planning or recorded commands (`crates/netem/src/lib.rs:203`, `crates/netem/src/lib.rs:208`, `crates/netem/src/lib.rs:236`).
3. The netem executor is a trait so the same command translation can target Linux `tc`, dry-run, or a test recorder without changing policy (`crates/netem/src/lib.rs:77`, `crates/netem/src/lib.rs:167`).
4. Disconnect and packet faults differ in state, atomicity, and rollback mechanics; the agent routes them to separate `Iptables` and `NetemAdapter` implementations (`crates/agent/src/lib.rs:257`, `crates/agent/src/firewall.rs:132`, `crates/netem/src/lib.rs:208`).
5. The agent's active-fault marker lives across connections, but the command executor socket handle is accepted per connection; mixing those lifetimes would make recovery state accidental (`crates/agent/src/lib.rs:417`, `crates/agent/src/lib.rs:429`).
6. Coordinator state and dispatch routing are centralized separately from target execution: the hub turns a controller command into a `LinkMessage::Dispatch` on the agent's reverse channel (or queues it while the agent is offline), while the agent crate owns actual fault application (`crates/coordinator/src/lib.rs:252`, `crates/coordinator/src/lib.rs:741`).
7. Reports require stable hashes, replay text, and evidence fields independent of whether the caller stores them in a file or coordinator memory (`crates/report/src/lib.rs:57`, `crates/report/src/lib.rs:87`, `crates/cli/src/main.rs:367`).
