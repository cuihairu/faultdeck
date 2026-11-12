# Agent 与注册

`faultdeck-agent` 是真正在目标主机上施加故障的常驻守护进程。一个 agent 管理一块网卡。

## 部署

```bash
sudo faultdeck-agent \
  --bind 0.0.0.0:7000 \
  --interface eth0 \
  --token <shared-secret> \
  --management 10.0.0.0/8 --management 192.168.0.0/16 \
  --iptables /usr/sbin/iptables
```

| 参数 | 默认 | 含义 |
|---|---|---|
| `--bind` | `127.0.0.1:7000` | TCP 监听地址 |
| `--interface` | `eth0` | `tc` 适配器操作的网卡 |
| `--token` | 不设(鉴权关闭) | 设置后每个请求都必须携带共享密钥 |
| `--management` | 空 | 可重复的保护网段;重叠即拒绝 |
| `--iptables` | 无 | iptables 二进制路径;**`disconnect` 必需** |
| `--coordinator` | 无 | 反向拨号的 coordinator `HOST:PORT`(link 模式,**Mode B**) |
| `--agent-id` | 无 | coordinator 注册表中的 id;**`--coordinator` 时必需** |
| `--heartbeat-ms` | `10000` | link 模式的心跳间隔(毫秒) |

link 模式下 agent **不再监听**控制连接 —— 而是主动出站拨号 coordinator(可穿双向 NAT),以 `--agent-id` 注册,发送心跳,执行下发的 `inject`/`rollback` 指令,并通过同一条连接回传结果;断线自动带退避重连。

启动警告:未配置任何 `--management` 网段时,agent 会打印 `warning: no management networks configured; agent may fault management traffic`;绑定非回环地址且未配置 `--token` 会拒绝启动(fail-closed),报错 `refusing to bind <addr> without a token; a daemon that can install DROP rules must fail closed — pass --token or bind a loopback address`。重要主机务必配置管理网段与 token。

默认网卡也可通过库 API `faultdeck_netem::default_device()` 发现(取 `/proc/net/route` 中 metric 最低的 UP 默认路由)。

## 请求处理

每条连接:读一行 → 可选的 `<token> ` 前缀与配置比对 → 载荷 ≤ 64 KiB → 解码为 `Request` → `inject` 先跑核心校验 → `process_request` 分发:

- `ping` → `pong`
- `inject` → 管理网段守卫 → 单故障检查 → `iptables`(disconnect)或 `tc netem`(latency/loss)
- `rollback` → 按当前已应用的类型撤销

响应为单行 JSON:`{"status":"ok"|"error","message":"…"}`。

生命周期说明:已应用故障的状态是**跨连接共享**的(`serve_on` 背后是一个 `Arc<Mutex<HashSet>>`,因此某条连接的 `rollback` 会清除另一条连接注入的故障),但守护进程重启后不保留故障记录。agent 被杀后请手动 `rollback` 或重新注入 —— 适配器在下次应用时会清理自己遗留的根 qdisc。

## Coordinator 注册

coordinator(默认端口 `7001`)维护 agent 注册表,协议复用同一套换行分帧 JSON 传输。每个请求都包裹在 `WireMessage` 信封 `{"token":…,"command":…}` 中;coordinator 以 `--token` 启动时,`token` 字段必须匹配(常量时间比较),否则以 `Unauthorized` 拒绝:

```text
{"token":"<secret-or-null>","command":{"cmd":"register","identity":{"id":"game-01","capabilities":["netem","firewall"],"last_seen_ms":1730000000000}}}
→ {"status":"registered","id":"game-01"}

{"token":"<secret-or-null>","command":{"cmd":"heartbeat","id":"game-01"}}
→ {"status":"ok","message":"heartbeat ok"}        // 未注册的 id 报 "not registered"

{"token":"<secret-or-null>","command":{"cmd":"list_agents"}}
→ {"status":"agents","agents":[ …AgentIdentity… (每项含 online:true|false) ],"available":["game-01"]}
// 在线判定:link 连接存活,或最后一次心跳在 30 s(HEARTBEAT_TIMEOUT_MS)内;
// 离线 agent 会被从 `available` 中剔除

{"token":"<secret-or-null>","command":{"cmd":"fetch_report","id":"game-01","scenario_hash":"<sha256>"}}
→ {"status":"report","report":{ …RunReport… }}    // 或 "report not found"

{"token":"<secret-or-null>","command":{"cmd":"inject","id":"game-01","injection":{ … }}}
→ {"status":"ok","message":"fault applied: …"}    // agent 在线:真实下发并回传执行结果
→ {"status":"queued","agent_id":"game-01","pending":1}  // agent 离线:指令持久入队,重连后自动下发
```

## 远程 CLI

`faultdeck` 二进制提供两类客户端。`remote` 直连 agent(Mode A);`hub` 连接 coordinator,由后者经 agent 反向链路转发指令(Mode B,不要求 agent 入站可达):

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

`hub inject`/`hub rollback` 最多等待 5 s 获取 agent 执行结果(经在线 link 下发);若 agent 离线,coordinator 会把指令入队并回复 `queued for {id} (pending N)` —— 该 agent 下次连上时自动补发。

传输层 crate(`faultdeck_agent::transport`)暴露两种连接模式:**Mode A**(agent 监听,控制端拨入)与 **Mode B**(agent 穿 NAT 拨出,带重试/退避)。Mode B 已端到端可用:`faultdeck-agent --coordinator HOST:PORT --agent-id ID` 维持反向链路,coordinator 沿该链路下发排队指令。
