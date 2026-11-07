# Architecture

FaultDeck is a Rust workspace. The current implementation ships **six built crates** plus one empty placeholder (`faultdeck-protocol` — the directory exists but contains no `Cargo.toml` and is **not** a workspace member yet). The planned toxiproxy backend and scenario YAML DSL from the [MVP plan](/design/mvp-plan) are not implemented; today's backend is `tc netem` + `iptables`.

## Crates

| crate | binary | responsibility |
|---|---|---|
| `faultdeck-core` | — | shared model: `FaultSpec` (latency / loss / disconnect), `Target`, `Injection`, intrinsic validation (`validate_injection`), management-network overlap check |
| `faultdeck-netem` | — | `tc` adapter: plans and executes `tc qdisc`/`filter` commands, preflight checks, rollback, `dry_run` executor |
| `faultdeck-agent` | `faultdeck-agent` | resident TCP daemon: token auth, request handling, iptables firewall (disconnect), single-active-fault state |
| `faultdeck-coordinator` | `faultdeck-coordinator` | multi-target hub: agent registry (register/heartbeat/list), report archive, queued inject/rollback commands |
| `faultdeck-report` | — | `Scenario`, `RunReport` (JSON/HTML), SHA-256 scenario hash, `compare_runs` history diff |
| `faultdeck-cli` | `faultdeck` | CLI entry: `faultdeck remote ping / inject / rollback` |
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
 │ registry: agents / reports / routes        │◀── framed JSON lines (multi-message
 │ AgentCommand: register, heartbeat,         │    connections, threaded accept)
 │   inject, rollback, fetch_report,          │
 │   list_agents                              │
 └────────────────────────────────────────────┘
```

## Wire protocol (CLI ↔ agent)

One TCP connection per CLI invocation. The client sends a single line; the agent answers with a single JSON line and closes.

- With token: `<token> {json}\n` — without: `{json}\n`
- Request: `{"cmd":"ping"}` | `{"cmd":"inject", ...Injection}` | `{"cmd":"rollback"}`
- Response: `{"status":"ok","message":"..."}` | `{"status":"error","message":"..."}`

## Wire protocol (coordinator)

Long-lived connections, newline-framed JSON lines in both directions (`faultdeck-agent::transport`). Tagged enums:

- `AgentCommand`: `register` | `heartbeat` | `inject` | `rollback` | `fetch_report` | `list_agents`
- `AgentResponse`: `ok` | `error` | `registered` | `agents` | `report`

State lives in a `Mutex<CoordinatorState>`: `agents` and `reports` are `BTreeMap`s, plus a `routes` map.

**Implementation status:** `inject`/`rollback` at the coordinator currently *queue* the command (the response is `queued for {id}`, and a report stub is archived under the scenario hash). Dispatch of queued commands to live agent connections is not wired yet; `register`/`heartbeat`/`list_agents`/`fetch_report` are fully functional in-process.

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

`crates/netem/src/lib.rs` translates validated latency/loss into ordered `tc qdisc` and `filter` commands through `NetemAdapter` (`crates/netem/src/lib.rs:172`, `crates/netem/src/lib.rs:199`, `crates/netem/src/lib.rs:204`). Its execution port is `CommandExecutor` (`crates/netem/src/lib.rs:73`), the command schema is `TcCommand` (`crates/netem/src/lib.rs:56`), and runtime inputs are the interface name plus the optional dry-run executor setting (`crates/netem/src/lib.rs:172`, `crates/netem/src/lib.rs:80`). It stores no state between calls and exposes no network protocol; implementation detail is in [Architecture](/architecture#failure-handling-in-the-netem-adapter).

### 3. Host execution and guard

`crates/agent/src/lib.rs` is the agent host: it accepts TCP, authenticates, guards management CIDRs, decodes requests, routes disconnect or netem, and keeps the active-fault marker in `Arc<Mutex<HashSet<String>>>` (`crates/agent/src/lib.rs:171`, `crates/agent/src/lib.rs:326`, `crates/agent/src/lib.rs:340`). Its config schema is `AgentConfig` (`crates/agent/src/lib.rs:90`), populated by `crates/agent/src/main.rs:9`; [Configuration](/configuration) documents those inputs. The `Iptables` backend in `crates/agent/src/firewall.rs:31` executes host rules, while protocol types remain in `crates/agent/src/lib.rs:109` and `crates/agent/src/lib.rs:117`; no dedicated protocol file exists.

### 4. Wire transport and topology

`crates/agent/src/transport.rs` owns bounded NDJSON framing plus Mode A listen/accept and Mode B reverse connect (`crates/agent/src/transport.rs:56`, `crates/agent/src/transport.rs:94`, `crates/agent/src/transport.rs:118`). It is stateless except accepted sockets. The framing schema is the generic `send_frame`/`read_frame` pair (`crates/agent/src/transport.rs:57`, `crates/agent/src/transport.rs:65`); topology prose is in [Agents](/remote-agents), and payload types live in the agent and coordinator layers rather than a dedicated protocol crate (`crates/protocol/src` is empty).

### 5. User boundary and report materialization

`crates/cli/src/main.rs` converts `remote ping/inject/rollback` arguments into shared `Injection` values, performs one bounded TCP exchange, optionally materializes JSON/HTML reports, and generates a replay command (`crates/cli/src/main.rs:96`, `crates/cli/src/main.rs:147`, `crates/cli/src/main.rs:200`, `crates/cli/src/main.rs:233`). It owns no server state; file output is caller-directed. The command schema is the clap tree at `crates/cli/src/main.rs:12`, `FAULTDECK_TIMEOUT_SECS` is the sole environment input (`crates/cli/src/main.rs:105`), and user-facing flags are documented in [Configuration](/configuration).

### 6. Fleet registry and control-plane state

`crates/coordinator/src/lib.rs` runs a threaded TCP hub, stores agents, reports, and logical routes in `Arc<Mutex<CoordinatorState>>`, and handles the tagged control protocol (`crates/coordinator/src/lib.rs:65`, `crates/coordinator/src/lib.rs:199`, `crates/coordinator/src/lib.rs:212`). All state is volatile. `AgentIdentity`, `AgentCommand`, `AgentResponse`, and `WireMessage` define that schema (`crates/coordinator/src/lib.rs:32`, `crates/coordinator/src/lib.rs:44`, `crates/coordinator/src/lib.rs:55`, `crates/coordinator/src/lib.rs:98`); `crates/coordinator/src/main.rs:4` supplies bind/token inputs documented in [Configuration](/configuration). [Agents](/remote-agents) carries the operational protocol prose.

### 7. Evidence and reproducibility model

`crates/report/src/lib.rs` owns `Scenario`, scenario hashing, `RunReport`, HTML rendering, timestamps, and report comparison (`crates/report/src/lib.rs:38`, `crates/report/src/lib.rs:49`, `crates/report/src/lib.rs:78`, `crates/report/src/lib.rs:110`, `crates/report/src/lib.rs:168`, `crates/report/src/lib.rs:182`). It is persistent-state-free; CLI and coordinator decide whether to retain output. Its Serde schema is the report model itself, HTML copy is generated in the same layer, and dedicated user prose is in [Reports](/reports).

### Why these layers are split

1. `core::Injection` is consumed by both direct CLI construction and coordinator validation, so semantic validation must not depend on either transport host (`crates/cli/src/main.rs:188`, `crates/coordinator/src/lib.rs:159`).
2. Netem exposes plan, inject, and rollback separately; the agent needs execution while adapters and tests need dry planning or recorded commands (`crates/netem/src/lib.rs:199`, `crates/netem/src/lib.rs:204`, `crates/netem/src/lib.rs:232`).
3. The netem executor is a trait so the same command translation can target Linux `tc`, dry-run, or a test recorder without changing policy (`crates/netem/src/lib.rs:73`, `crates/netem/src/lib.rs:163`).
4. Disconnect and packet faults differ in state, atomicity, and rollback mechanics; the agent routes them to separate `Iptables` and `NetemAdapter` implementations (`crates/agent/src/lib.rs:196`, `crates/agent/src/firewall.rs:117`, `crates/netem/src/lib.rs:204`).
5. The agent's active-fault marker lives across connections, but the command executor socket handle is accepted per connection; mixing those lifetimes would make recovery state accidental (`crates/agent/src/lib.rs:340`, `crates/agent/src/lib.rs:341`).
6. Coordinator state and control routing are centralized separately from target execution because inject currently archives and queues without forwarding to an agent (`crates/coordinator/src/lib.rs:159`, `crates/coordinator/src/lib.rs:176`).
7. Reports require stable hashes, replay text, and evidence fields independent of whether the caller stores them in a file or coordinator memory (`crates/report/src/lib.rs:49`, `crates/report/src/lib.rs:92`, `crates/cli/src/main.rs:233`).
