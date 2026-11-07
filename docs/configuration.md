# Configuration

FaultDeck has no config files — everything is passed as CLI flags/args at process start. Values below are the defaults from the source.

## faultdeck-agent

| flag | default | notes |
|---|---|---|
| `--bind ADDR` | `127.0.0.1:7000` | TCP listen address |
| `--token TOKEN` | unset → auth disabled | when set, every request line must start with `<token> ` |
| `--interface IFACE` | `eth0` | interface for the `tc` adapter |
| `--management CIDR` | empty | repeatable; protected networks, overlap rejects injection |
| `--iptables PATH` | none | required for `disconnect`; without it the agent answers `iptables missing` |
| `--coordinator HOST:PORT` | none | link mode: reverse-dial the coordinator instead of listening |
| `--agent-id ID` | none | registry id; required with `--coordinator` |
| `--heartbeat-ms MS` | `10000` | link-mode heartbeat interval (link mode only) |

Behavioral constants (compiled in, not configurable):

| constant | value | where |
|---|---|---|
| max request payload | 64 KiB | `faultdeck_agent::MAX_INJECTION_BYTES` |
| pilot qdisc handle | `100:` | `faultdeck_netem::PILOT_QDISC_HANDLE` |
| netem qdisc handle | `50:` | adapter-internal |
| max latency | 86 400 000 ms (24 h) | `faultdeck_core::MAX_LATENCY_MS` |
| iptables chain | `FAULTDECK` in table `mangle` | `faultdeck_agent::firewall::CHAIN` |

`AgentConfig` (the library struct behind the flags) also serializes to JSON, so agents can be constructed programmatically.

## faultdeck-coordinator

```bash
faultdeck-coordinator --bind 127.0.0.1:7001 [--token <shared-secret>]
```

`--bind` defaults to `127.0.0.1:7001`. `--token` is optional; when set, every
wire message must carry the matching token (see `remote-agents.md`). State
(registry, report archive, live links, per-agent command queues) is in-memory.

## faultdeck (CLI)

| flag | applies to | default | notes |
|---|---|---|---|
| `--host` | all `remote`/`hub` subcommands | required | agent (`remote`) or coordinator (`hub`) address |
| `--port` | all `remote`/`hub` subcommands | `7000` (`remote`), `7001` (`hub`) | server port |
| `--token` | all `remote`/`hub` subcommands | none | prefixed to the request line |
| `--agent` | `hub inject`/`hub rollback` | required | target agent id in the coordinator registry |
| `--fault` | `inject` | required | `latency` \| `loss` \| `disconnect` |
| `--destination` | `inject` | required | IPv4 CIDR |
| `--port-spec` | `inject` | none | optional destination-port match |
| `--milliseconds` | `inject --fault latency` | required by that fault | delay |
| `--percent` | `inject --fault loss` | required by that fault | loss percent |
| `--scenario-name` | `inject` | `"scenario"` | report label |
| `--report-json` | `remote inject` | none | write JSON report locally |
| `--report-html` | `remote inject` | none | write HTML report locally |

One environment variable exists: `FAULTDECK_TIMEOUT_SECS` overrides the CLI's
5-second default read/write timeout on agent connections
(`crates/cli/src/main.rs`). There is no config file lookup, and the agent and
coordinator have no options beyond the flags above.
