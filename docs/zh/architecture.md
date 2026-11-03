# 架构

FaultDeck 是一个 Rust workspace。当前实现包含 **6 个已构建 crate**,外加一个空占位(`faultdeck-protocol` —— 目录存在,但没有 `Cargo.toml`,**尚未加入 workspace**)。[MVP 计划](/zh/design/mvp-plan)中的 toxiproxy 后端与场景 YAML DSL 尚未实现;如今的实现后端是 `tc netem` + `iptables`。

## Crate 一览

| crate | 二进制 | 职责 |
|---|---|---|
| `faultdeck-core` | — | 共享模型:`FaultSpec`(latency / loss / disconnect)、`Target`、`Injection`、内在校验(`validate_injection`)、管理网段重叠检查 |
| `faultdeck-netem` | — | `tc` 适配器:规划并执行 `tc qdisc`/`filter` 命令,preflight 检查,回滚,`dry_run` 执行器 |
| `faultdeck-agent` | `faultdeck-agent` | 常驻 TCP 守护进程:token 鉴权、请求处理、iptables 防火墙(disconnect)、单故障状态 |
| `faultdeck-coordinator` | `faultdeck-coordinator` | 多目标中枢:agent 注册表(register/heartbeat/list)、报告归档、排队中的 inject/rollback 命令 |
| `faultdeck-report` | — | `Scenario`、`RunReport`(JSON/HTML)、SHA-256 场景哈希、`compare_runs` 历史 diff |
| `faultdeck-cli` | `faultdeck` | CLI 入口:`faultdeck remote ping / inject / rollback` |
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
 │ 注册表: agents / reports / routes           │◀── 换行分帧 JSON(多消息长连接,
 │ AgentCommand: register, heartbeat,         │    线程化 accept)
 │   inject, rollback, fetch_report,          │
 │   list_agents                              │
 └────────────────────────────────────────────┘
```

## 线协议(CLI ↔ agent)

每次 CLI 调用一条 TCP 连接。客户端发一行,agent 应答一行 JSON 后关闭。

- 带 token:`<token> {json}\n` —— 不带:`{json}\n`
- 请求:`{"cmd":"ping"}` | `{"cmd":"inject", …Injection}` | `{"cmd":"rollback"}`
- 响应:`{"status":"ok","message":"…"}` | `{"status":"error","message":"…"}`

## 线协议(coordinator)

长连接,双向按行分帧的 JSON(`faultdeck-agent::transport`)。标签枚举:

- `AgentCommand`:`register` | `heartbeat` | `inject` | `rollback` | `fetch_report` | `list_agents`
- `AgentResponse`:`ok` | `error` | `registered` | `agents` | `report`

状态保存在 `Mutex<CoordinatorState>`:`agents`、`reports` 为 `BTreeMap`,另有 `routes` 映射。

**实现状态:** coordinator 的 `inject`/`rollback` 目前只是**入队**(响应为 `queued for {id}`,并按场景哈希归档一份报告桩)。向在线 agent 连接的分发尚未接线;`register`/`heartbeat`/`list_agents`/`fetch_report` 在进程内已完整可用。

## netem 适配器的失败处理

1. `plan()` 只校验并返回命令列表,不执行。
2. `inject()` 先跑 preflight `tc qdisc show` —— 若网卡根 qdisc 不是 FaultDeck 的 `100:` 句柄,拒绝操作。
3. 应用前先清理上一次运行遗留的 FaultDeck 根 qdisc。
4. 应用过程中任何命令失败,适配器立即下发回滚命令。
5. `rollback()` 删除根 qdisc;`tc` 的「找不到」类错误按已清理处理(幂等)。

disconnect 故障完全绕过 `tc` —— 见 [Netem 故障](/zh/netem-faults#disconnect)。
