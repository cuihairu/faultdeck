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
(registry, report archive, routes) is in-memory.

## faultdeck (CLI)

| flag | applies to | default | notes |
|---|---|---|---|
| `--host` | all `remote` subcommands | required | agent address |
| `--port` | all `remote` subcommands | `7000` | agent port |
| `--token` | all `remote` subcommands | none | prefixed to the request line |
| `--fault` | `inject` | required | `latency` \| `loss` \| `disconnect` |
| `--destination` | `inject` | required | IPv4 CIDR |
| `--port-spec` | `inject` | none | optional destination-port match |
| `--milliseconds` | `inject --fault latency` | required by that fault | delay |
| `--percent` | `inject --fault loss` | required by that fault | loss percent |
| `--scenario-name` | `inject` | `"scenario"` | report label |
| `--report-json` | `inject` | none | write JSON report locally |
| `--report-html` | `inject` | none | write HTML report locally |

One environment variable exists: `FAULTDECK_TIMEOUT_SECS` overrides the CLI's
5-second default read/write timeout on agent connections
(`crates/cli/src/main.rs`). There is no config file lookup, and the agent and
coordinator have no options beyond the flags above.
