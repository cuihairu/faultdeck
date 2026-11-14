# Glossary

[中文术语表](/zh/glossary)。 These terms describe the implemented responsibility layers, not directory names. Architecture sections are in [Architecture](/architecture#source-layer-map).

| 术语 | English | 定义 | file:line |
|---|---|---|---|
| 故障规格 | `FaultSpec` | A tagged model for latency, loss, or disconnect semantics shared by the user boundary, coordinator, agent, and adapters. | `crates/core/src/lib.rs:26` |
| 故障目标 | `Target` | The optional IPv4 source, destination, and destination-port selectors that scope one injection. | `crates/core/src/lib.rs:44` |
| 注入请求 | `Injection` | One fault plus one target; this pair is the shared execution contract. | `crates/core/src/lib.rs:82` |
| 内在校验 | intrinsic validation | Checks fault ranges, non-empty targets, and IPv6 support before backend-specific behavior runs. | `crates/core/src/lib.rs:108` |
| 管理网段保护 | management-network protection | Rejects a target whose networks overlap operator-provided protected CIDRs. | `crates/core/src/lib.rs:142` |
| 执行器端口 | `CommandExecutor` | The netem adapter's boundary for running or recording `tc` commands; production and test executors implement it. | `crates/netem/src/lib.rs:77` |
| tc 命令 | `TcCommand` | A reproducible program-and-arguments representation of one traffic-control operation. | `crates/netem/src/lib.rs:61` |
| netem 适配器 | `NetemAdapter` | Plans, applies, and rolls back qdisc/filter state for latency and loss on one interface. | `crates/netem/src/lib.rs:177` |
| pilot qdisc | pilot qdisc | FaultDeck-owned root qdisc handle `100:` used to recognize and safely remove only its own traffic-control state. | `crates/netem/src/lib.rs:13` |
| 断连路由 | disconnect routing | The agent routes a disconnect injection to iptables instead of `tc`; other faults go to netem. | `crates/agent/src/lib.rs:257` |
| iptables 链 | iptables chain | The `FAULTDECK` mangle chain and its `INPUT`/`OUTPUT`/`FORWARD` jumps carry disconnect DROP rules in both directions. | `crates/agent/src/firewall.rs:9` |
| 断连适配器 | disconnect adapter | `Iptables` ensures the chain, installs DROP rules, and performs idempotent rollback. | `crates/agent/src/firewall.rs:37` |
| Agent 配置 | `AgentConfig` | Process-start settings: bind address, optional token, target interface, and management networks. | `crates/agent/src/lib.rs:137` |
| 单活动故障 | single active fault | Agent-host state that rejects a second injection until rollback clears the adapter marker. | `crates/agent/src/lib.rs:252` |
| Agent 线请求 | agent wire request | The tagged `ping`, `inject`, or `rollback` payload decoded by the agent host. | `crates/agent/src/lib.rs:157` |
| Agent 线响应 | agent wire response | The tagged `ok` or `error` result returned to the CLI. | `crates/agent/src/lib.rs:165` |
| 令牌鉴权 | token authentication | Optional shared-secret comparison at each process boundary; an unset token means auth is disabled. | `crates/agent/src/lib.rs:170` |
| NDJSON 帧 | NDJSON frame | A newline-delimited JSON value with a hard 64 KiB frame ceiling. | `crates/agent/src/transport.rs:27` |
| Mode A | Mode A | Agent-listens topology: the controller opens one TCP stream to the resident target agent. | `crates/agent/src/transport.rs:94` |
| Mode B | Mode B | Controller-listens reverse-connect topology with retry and backoff for NAT traversal. | `crates/agent/src/transport.rs:118` |
| 中枢信封 | `WireMessage` | The coordinator envelope carrying an optional token and one control command. | `crates/coordinator/src/lib.rs:270` |
| 中枢命令 | `AgentCommand` | Register, heartbeat, inject, rollback, fetch-report, or list-agents operations accepted by the coordinator. | `crates/coordinator/src/lib.rs:67` |
| 中枢响应 | `AgentResponse` | Coordinator acknowledgement, queue receipt, registry listing, report, or error result. | `crates/coordinator/src/lib.rs:92` |
| Agent 身份 | `AgentIdentity` | An agent ID, capability list, last-seen timestamp, and list-time online flag stored by the coordinator. | `crates/coordinator/src/lib.rs:49` |
| 心跳 | heartbeat | Updates one registered agent's `last_seen_ms`; freshness feeds the online check. | `crates/coordinator/src/lib.rs:207` |
| Agent 链路 | agent link | The agent's outbound connection to the coordinator; `LinkMessage::Dispatch` goes down it, `Result`/`Heartbeat` come back up. | `crates/coordinator/src/lib.rs:154` |
| 在线状态 | online status | Link liveness OR a heartbeat within `HEARTBEAT_TIMEOUT_MS` (30 s); offline agents are excluded from `available`. | `crates/coordinator/src/lib.rs:243` |
| 指令入队 | queued command | When the target agent is offline, inject/rollback persist in its queue and are dispatched on its next registration. | `crates/coordinator/src/lib.rs:252` |
| 场景 | `Scenario` | A named injection whose stable serialization is the input to hashing and reporting. | `crates/report/src/lib.rs:41` |
| 场景哈希 | scenario hash | The SHA-256 identity of a scenario, used for report lookup and comparison. | `crates/report/src/lib.rs:57` |
| 运行报告 | `RunReport` | The canonical result model containing scenario identity, timestamps, evidence, and replay text. | `crates/report/src/lib.rs:87` |
| 回放命令 | replay command | A shell-quoted CLI invocation generated by the user boundary and stored in the report. | `crates/cli/src/main.rs:367` |
| 恢复证据 | restore evidence | A timestamped post-rollback check with pass/fail and detail fields. | `crates/report/src/lib.rs:72` |
| 外部断言 | external assertion | A named externally observed result attached to a run report. | `crates/report/src/lib.rs:80` |
| 历史 diff | historical diff | A structural comparison of two reports' event, evidence, and assertion collections. | `crates/report/src/lib.rs:190` |

