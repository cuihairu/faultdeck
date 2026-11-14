# 架构

FaultDeck 是一个 Rust workspace。当前实现包含 **6 个已构建 crate**,外加一个空占位(`faultdeck-protocol` —— 目录存在,但没有 `Cargo.toml`,**尚未加入 workspace**)。[MVP 计划](/zh/design/mvp-plan)中的 toxiproxy 后端与场景 YAML DSL 尚未实现;如今的实现后端是 `tc netem` + `iptables`。

## Crate 一览

| crate | 二进制 | 职责 |
|---|---|---|
| `faultdeck-core` | — | 共享模型:`FaultSpec`(latency / loss / disconnect)、`Target`、`Injection`、内在校验(`validate_injection`)、管理网段重叠检查 |
| `faultdeck-netem` | — | `tc` 适配器:规划并执行 `tc qdisc`/`filter` 命令,preflight 检查,回滚,`dry_run` 执行器 |
| `faultdeck-agent` | `faultdeck-agent` | 常驻 TCP 守护进程:token 鉴权、请求处理、iptables 防火墙(disconnect)、单故障状态 |
| `faultdeck-coordinator` | `faultdeck-coordinator` | 多目标中枢:agent 注册表(register/heartbeat/list 含在线状态)、报告归档、经 agent 反向链路在线下发 inject/rollback,离线 agent 指令入队 |
| `faultdeck-report` | — | `Scenario`、`RunReport`(JSON/HTML)、SHA-256 场景哈希、`compare_runs` 历史 diff |
| `faultdeck-cli` | `faultdeck` | CLI 入口:`faultdeck remote ping / inject / rollback`(直连 agent)与 `faultdeck hub list / inject / rollback`(经 coordinator) |
| `faultdeck-protocol` | — | *占位,不在 workspace 中* |

## 交互总览

```
 faultdeck CLI                faultdeck-agent                目标主机
 ┌──────────────┐  TCP 换行   ┌──────────────────┐   tc      ┌──────────┐
 │ remote ping  │  分帧 JSON  │ token 鉴权        │──qdisc──▶ │  eth0    │
 │ remote inject│             │ 管理网段守卫       │──filter─▶ │ (netem)  │
 │ remote       │             │ validate_injection│           └──────────┘
 │   rollback   │             │                  │  iptables ┌──────────┐
 └──────────────┘             │ firewall (mangle)│──DROP────▶│ disconnect│
                              └──────────────────┘  -t mangle└──────────┘

 faultdeck-coordinator(默认 :7001)
 ┌────────────────────────────────────────────┐
 │ 注册表: agents / reports / links / queues   │◀── 换行分帧 JSON(多消息长连接,
 │ AgentCommand: register, heartbeat,         │    线程化 accept)
 │   inject, rollback, fetch_report,          │◀── agent 反向链路:先 RegisterEnvelope,
 │   list_agents                              │    再 LinkMessage::Dispatch → Result /
 │                                            │    Heartbeat 同链路往返
 └────────────────────────────────────────────┘
```

## 线协议(CLI ↔ agent)

每次 CLI 调用一条 TCP 连接。客户端发一行,agent 应答一行 JSON 后关闭。

- 带 token:`<token> {json}\n` —— 不带:`{json}\n`
- 请求:`{"cmd":"ping"}` | `{"cmd":"inject", …Injection}` | `{"cmd":"rollback"}`
- 响应:`{"status":"ok","message":"…"}` | `{"status":"error","message":"…"}`

## 线协议(coordinator)

长连接,双向按行分帧的 JSON(`faultdeck-agent::transport`)。同一监听端口承载两种会话,按首帧区分:

- **控制端会话** —— 每条命令一个 `WireMessage` 信封。`AgentCommand`:`register` | `heartbeat` | `inject` | `rollback` | `fetch_report` | `list_agents`;`AgentResponse`:`ok` | `error` | `registered` | `queued` | `agents` | `report`。
- **Agent 链路** —— agent 主动出站拨号 coordinator(可穿双向 NAT),首帧发 `RegisterEnvelope`(`crates/agent/src/link.rs:46`);此后 coordinator 沿同一条连接写 `LinkMessage::Dispatch`,agent 以 `LinkMessage::Result` 应答并周期发送 `LinkMessage::Heartbeat`。

状态保存在 `Mutex<CoordinatorState>`:`agents`、`reports`、`links`(存活反向连接)、每 agent 的 `queues` 均为 `BTreeMap`。

**实现状态:** coordinator 的 `inject`/`rollback` 先校验、归档报告桩,然后**真实下发** —— 存活链路立即收到指令,agent 的执行结果回传给等待中的控制端(受 `DEFAULT_DISPATCH_TIMEOUT` 5 s 约束)并合入归档报告;agent 离线时指令持久入队,在其下次注册时补发。`list_agents` 依据链路存活或心跳新鲜度(`HEARTBEAT_TIMEOUT_MS` 30 s)标注每项 `AgentIdentity.online`,离线 agent 从 `available` 中剔除。

## netem 适配器的失败处理

1. `plan()` 只校验并返回命令列表,不执行。
2. `inject()` 先跑 preflight `tc qdisc show` —— 若网卡根 qdisc 不是 FaultDeck 的 `100:` 句柄,拒绝操作。
3. 应用前先清理上一次运行遗留的 FaultDeck 根 qdisc。
4. 应用过程中任何命令失败,适配器立即下发回滚命令。
5. `rollback()` 删除根 qdisc;`tc` 的「找不到」类错误按已清理处理(幂等)。

disconnect 故障完全绕过 `tc` —— 见 [Netem 故障](/zh/netem-faults#disconnect)。

## 源码分层解说

源码职责层与 crate 目录名相互独立。术语已同步到[术语表](/zh/glossary)。

### 1. 共享语义与安全模型

`crates/core/src/lib.rs` 拥有 `FaultSpec`、`Target`、`Injection`、内在校验和管理网段重叠检查(`crates/core/src/lib.rs:26`、`crates/core/src/lib.rs:44`、`crates/core/src/lib.rs:82`、`crates/core/src/lib.rs:108`、`crates/core/src/lib.rs:142`)。该层无持久状态、无网络 I/O。schema 是 Rust Serde 模型和 `crates/core/src/lib.rs:88` 的内在校验错误;故障语义文案在 [Netem 故障](/zh/netem-faults),没有专属协议文件。

### 2. 策略到后端翻译层

`crates/netem/src/lib.rs` 通过 `NetemAdapter` 把已校验的延迟/丢包翻译成有序 `tc qdisc` 和 `filter` 命令(`crates/netem/src/lib.rs:172`、`crates/netem/src/lib.rs:199`、`crates/netem/src/lib.rs:204`)。执行端口是 `CommandExecutor`(`crates/netem/src/lib.rs:73`),命令 schema 是 `TcCommand`(`crates/netem/src/lib.rs:56`),运行输入为网卡名和可选 dry-run 执行器设置(`crates/netem/src/lib.rs:172`、`crates/netem/src/lib.rs:80`)。调用之间无状态、无网络协议;实现细节见 [架构](/zh/architecture#netem-适配器的失败处理)。

### 3. 宿主执行与防护层

`crates/agent/src/lib.rs` 是 Agent 宿主:接收 TCP、鉴权、防护管理网段、解码请求、路由 disconnect 或 netem,并用 `Arc<Mutex<HashSet<String>>>` 保存活动故障标记(`crates/agent/src/lib.rs:171`、`crates/agent/src/lib.rs:326`、`crates/agent/src/lib.rs:340`)。配置 schema 是 `AgentConfig`(`crates/agent/src/lib.rs:90`),由 `crates/agent/src/main.rs:9` 填充;这些输入记录在[配置](/zh/configuration)。`crates/agent/src/firewall.rs:31` 的 `Iptables` 执行主机规则;协议类型仍在 `crates/agent/src/lib.rs:109` 和 `crates/agent/src/lib.rs:117`,没有专属协议文件。

### 4. 线传输与拓扑层

`crates/agent/src/transport.rs` 拥有带界限的 NDJSON 分帧、Mode A 监听/接受和 Mode B 反向连接(`crates/agent/src/transport.rs:56`、`crates/agent/src/transport.rs:94`、`crates/agent/src/transport.rs:118`)。除已接受 socket 外无状态。分帧 schema 是通用 `send_frame`/`read_frame`(`crates/agent/src/transport.rs:57`、`crates/agent/src/transport.rs:65`)。在此分帧之上,`crates/agent/src/link.rs` 实现可用的 Mode B 链路会话 —— 注册信封、心跳线程、分发泵,以及可重连的 `run_link` 循环(`crates/agent/src/link.rs:77`、`crates/agent/src/link.rs:140`、`crates/agent/src/link.rs:184`)。拓扑文案在 [Agents](/zh/remote-agents),载荷类型分属 agent 与 coordinator 层,`crates/protocol/src` 为空目录。

### 5. 用户入口与报告物化层

`crates/cli/src/main.rs` 把 `remote ping/inject/rollback` 与 `hub list/inject/rollback` 参数转换为共享 `Injection`,与 agent 或 coordinator 执行有超时的 TCP 交互,按需物化 JSON/HTML 报告并生成回放命令(`crates/cli/src/main.rs:136`、`crates/cli/src/main.rs:165`、`crates/cli/src/main.rs:227`、`crates/cli/src/main.rs:341`)。它没有服务端状态;文件输出由调用参数指定。命令 schema 是 `crates/cli/src/main.rs:22` 的 clap 树,唯一环境输入是 `FAULTDECK_TIMEOUT_SECS`(`crates/cli/src/main.rs:236`),用户旗标记录在[配置](/zh/configuration)。

### 6. 机群注册与控制面状态层

`crates/coordinator/src/lib.rs` 运行多线程 TCP 中枢,把 agents、reports、存活链路 links 和每 agent 队列 queues 保存在 `Arc<Mutex<CoordinatorState>>`,同时处理控制端会话与 agent 反向链路(`crates/coordinator/src/lib.rs:156`、`crates/coordinator/src/lib.rs:270`、`crates/coordinator/src/lib.rs:455`、`crates/coordinator/src/lib.rs:503`)。全部状态易失。`AgentIdentity`、`AgentCommand`、`AgentResponse` 和 `WireMessage` 定义该协议(`crates/coordinator/src/lib.rs:51`、`crates/coordinator/src/lib.rs:69`、`crates/coordinator/src/lib.rs:80`、`crates/coordinator/src/lib.rs:235`);`crates/coordinator/src/main.rs:4` 提供 bind/token 输入并记录在[配置](/zh/configuration)。操作协议文案在 [Agents](/zh/remote-agents)。

### 7. 证据与可复现模型层

`crates/report/src/lib.rs` 拥有 `Scenario`、场景哈希、`RunReport`、HTML 渲染、时间戳和报告比较(`crates/report/src/lib.rs:38`、`crates/report/src/lib.rs:49`、`crates/report/src/lib.rs:78`、`crates/report/src/lib.rs:110`、`crates/report/src/lib.rs:168`、`crates/report/src/lib.rs:182`)。该层无持久状态;CLI 和 coordinator 决定是否保留输出。其 Serde schema 就是报告模型本身,HTML 文案在同一层生成,专题文案在[场景报告](/zh/reports)。

### 8. 面向 AI agent 的壳层

`crates/faultdeck-mcp/src/main.rs` 是 MCP server(stdio,官方 `rmcp` SDK),让 AI agent 驱动机群。它暴露五个 tool —— `list_targets`、`apply_fault`、`clear_fault`、`run_scenario`、`read_report` —— 说的是 coordinator wire 协议(`WireMessage`/`AgentCommand`),走与 CLI hub 路径同一套 `faultdeck_agent::transport` 分帧(`crates/faultdeck-mcp/src/main.rs`)。该层没有任何注入实现:校验留在 coordinator,施加留在 agent,故障类型可用性由各 agent 经 `list_targets` 自己上报。

### 9. 桌面壳层

`crates/desktop/src/lib.rs` 是 Tauri 桌面 UI:车群/注入/报告三视图,Rust 侧调用 `coordinator_list`、`coordinator_inject`、`coordinator_rollback`、`coordinator_report`(`crates/desktop/src/lib.rs:162`、`crates/desktop/src/lib.rs:172`、`crates/desktop/src/lib.rs:193`、`crates/desktop/src/lib.rs:204`)。Rust 命令直说 coordinator wire 协议,因此 UI 不引入第二条控制面链路。

### 拆分理由

1. `core::Injection` 同时被 CLI 直接构造和 coordinator 校验消费,语义校验不能依赖任一传输宿主(`crates/cli/src/main.rs:136`、`crates/coordinator/src/lib.rs:309`)。
2. netem 分别暴露 plan、inject、rollback;Agent 需要执行,而适配器和测试需要 dry 规划或记录命令(`crates/netem/src/lib.rs:199`、`crates/netem/src/lib.rs:204`、`crates/netem/src/lib.rs:232`)。
3. netem 执行器是 trait,同一命令翻译可指向 Linux `tc`、dry-run 或测试记录器,而不改策略(`crates/netem/src/lib.rs:73`、`crates/netem/src/lib.rs:163`)。
4. Disconnect 和包级故障的状态、原子性与回滚机制不同;Agent 将它们路由到独立的 `Iptables` 与 `NetemAdapter` 实现(`crates/agent/src/lib.rs:196`、`crates/agent/src/firewall.rs:117`、`crates/netem/src/lib.rs:204`)。
5. Agent 的活动故障标记跨连接存在,但命令执行 socket 按连接接受;合并这两个生命周期会让恢复状态变成偶然实现(`crates/agent/src/lib.rs:340`、`crates/agent/src/lib.rs:341`)。
6. coordinator 状态与分发路由同目标执行分离:中枢把控制端指令转成 agent 反向链路上的 `LinkMessage::Dispatch`(agent 离线时入队),而真正施加故障由 agent crate 负责(`crates/coordinator/src/lib.rs:351`、`crates/coordinator/src/lib.rs:660`)。
7. 报告需要稳定哈希、回放文本和证据字段,不依赖调用者把它存文件还是 coordinator 内存(`crates/report/src/lib.rs:49`、`crates/report/src/lib.rs:92`、`crates/cli/src/main.rs:233`)。
