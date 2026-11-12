# Agents

`faultdeck-agent` is the resident daemon that actually applies faults on a target host. One agent owns one network interface.

## Deployment

```bash
sudo faultdeck-agent \
  --bind 0.0.0.0:7000 \
  --interface eth0 \
  --token <shared-secret> \
  --management 10.0.0.0/8 --management 192.168.0.0/16 \
  --iptables /usr/sbin/iptables
```

| flag | default | meaning |
|---|---|---|
| `--bind` | `127.0.0.1:7000` | TCP listen address |
| `--interface` | `eth0` | interface the `tc` adapter operates on |
| `--token` | none (auth disabled) | shared secret required on every request when set |
| `--management` | empty | repeatable protected CIDR; overlaps are rejected |
| `--iptables` | none | path to the iptables binary; **required for `disconnect`** |
| `--coordinator` | none | coordinator `HOST:PORT` to reverse-dial (link mode, **Mode B**) |
| `--agent-id` | none | id used in the coordinator registry; **required with `--coordinator`** |
| `--heartbeat-ms` | `10000` | link heartbeat interval in link mode |

In link mode the agent does **not** listen for control connections — it dials the coordinator outbound (works behind double-NAT), registers with `--agent-id`, sends heartbeats, executes dispatched `inject`/`rollback` commands, and replies with results over the same connection, reconnecting with backoff if the link drops.

Startup warning: if no `--management` networks are configured, the agent prints `warning: no management networks configured; agent may fault management traffic`. Binding a non-loopback address without `--token` is refused (fail-closed): startup exits with `refusing to bind <addr> without a token; a daemon that can install DROP rules must fail closed — pass --token or bind a loopback address`. Always configure management networks and a token on hosts you care about.

The default interface can also be discovered programmatically via `faultdeck_netem::default_device()` (lowest-metric UP default route from `/proc/net/route`).

## Request handling

Each connection: read one line → the optional `<token> ` prefix is checked against the config → payload must be ≤ 64 KiB → decoded as a `Request` → for `inject`, core validation runs → `process_request` dispatches:

- `ping` → `pong`
- `inject` → management guard → single-fault check → `iptables` (disconnect) or `tc netem` (latency/loss)
- `rollback` → undo by what is currently applied

Responses are `{"status":"ok"|"error","message":"…"}` on a single JSON line.

Note on lifecycle: the applied-fault state is shared across connections (an `Arc<Mutex<HashSet>>` behind `serve_on`, so a rollback on one connection clears what another connection injected), but the daemon does not track faults across restarts. If the agent is killed, run `rollback` manually or re-inject — the adapter cleans its own leftover root qdisc on the next apply.

## Coordinator registration

The coordinator (default port `7001`) keeps a registry of agents. The protocol
reuses the same newline-framed JSON transport. Every request is wrapped in a
`WireMessage` envelope `{"token":…,"command":…}`; when the coordinator is
started with `--token`, the `token` field must match (compared in constant
time) or the request is rejected with `Unauthorized`:

```text
{"token":"<secret-or-null>","command":{"cmd":"register","identity":{"id":"game-01","capabilities":["netem","firewall"],"last_seen_ms":1730000000000}}}
→ {"status":"registered","id":"game-01"}

{"token":"<secret-or-null>","command":{"cmd":"heartbeat","id":"game-01"}}
→ {"status":"ok","message":"heartbeat ok"}        // "not registered" error if unknown id

{"token":"<secret-or-null>","command":{"cmd":"list_agents"}}
→ {"status":"agents","agents":[ …AgentIdentity… (each carries online:true|false) ],"available":["game-01"]}
// an agent is online while its link is live or its last heartbeat is within
// 30 s (HEARTBEAT_TIMEOUT_MS); offline agents are excluded from `available`

{"token":"<secret-or-null>","command":{"cmd":"fetch_report","id":"game-01","scenario_hash":"<sha256>"}}
→ {"status":"report","report":{ …RunReport… }}    // or error "report not found"

{"token":"<secret-or-null>","command":{"cmd":"inject","id":"game-01","injection":{ … }}}
→ {"status":"ok","message":"fault applied: …"}    // agent online: real dispatch, execution result returned
→ {"status":"queued","agent_id":"game-01","pending":1}  // agent offline: command persists and is delivered on reconnect
```

## Remote CLI

The `faultdeck` binary ships two client families. `remote` speaks to an agent directly (Mode A); `hub` speaks to the coordinator, which forwards commands to agents over their reverse links (Mode B, no inbound reachability to the agent required):

```bash
faultdeck remote ping     --host AGENT --port 7000 [--token T]
faultdeck remote inject   --host AGENT --port 7000 [--token T] \
    --fault latency|loss|disconnect --destination CIDR [--port-spec P] \
    [--milliseconds MS] [--percent PCT] \
    [--scenario-name NAME] [--report-json F] [--report-html F]
faultdeck remote rollback --host AGENT --port 7000 [--token T]

faultdeck hub list     --host COORD --port 7001 [--token T]
faultdeck hub inject   --host COORD --port 7001 [--token T] --agent game-01 \
    --fault latency|loss|disconnect --destination CIDR [--port-spec P] \
    [--milliseconds MS] [--percent PCT] [--scenario-name NAME]
faultdeck hub rollback --host COORD --port 7001 [--token T] --agent game-01
```

`hub inject`/`hub rollback` wait up to 5 s for the agent's execution result (dispatch to a live link); if the agent is offline the coordinator queues the command and replies `queued for {id} (pending N)` — it is delivered automatically the next time that agent connects.

The transport crate (`faultdeck_agent::transport`) exposes both connection modes: **Mode A** (agent listens, controller dials in) and **Mode B** (agent dials out through NAT with retry/backoff). Mode B is live end to end: `faultdeck-agent --coordinator HOST:PORT --agent-id ID` maintains the reverse link, and the coordinator dispatches queued commands down it.
