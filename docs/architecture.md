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
