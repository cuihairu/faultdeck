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

启动警告:未配置任何 `--management` 网段时,agent 会打印 `warning: no management networks configured; agent may fault management traffic`;绑定非回环地址且未配置 `--token` 时也会告警:`warning: binding <addr> without a token; any reachable host can control the agent`。重要主机务必配置管理网段与 token。

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
→ {"status":"agents","agents":[ …AgentIdentity… ]}

{"token":"<secret-or-null>","command":{"cmd":"fetch_report","id":"game-01","scenario_hash":"<sha256>"}}
→ {"status":"report","report":{ …RunReport… }}    // 或 "report not found"

{"token":"<secret-or-null>","command":{"cmd":"inject","id":"game-01","injection":{ … }}}
→ {"status":"ok","message":"queued for game-01"}  // 仅入队;在线分发尚未接线
```

## 远程 CLI

`faultdeck` 二进制目前是唯一随仓库发布的客户端,且只与 agent 直接通信(尚无 coordinator 子命令):

```bash
faultdeck remote ping     --host AGENT --port 7000 [--token T]
faultdeck remote inject   --host AGENT --port 7000 [--token T] \
    --fault latency|loss|disconnect --destination CIDR [--port-spec P] \
    [--milliseconds MS] [--percent PCT] \
    [--scenario-name NAME] [--report-json F] [--report-html F]
faultdeck remote rollback --host AGENT --port 7000 [--token T]
```

传输层 crate(`faultdeck_agent::transport`)还为未来客户端暴露两种连接模式:**Mode A**(agent 监听,控制端拨入)与 **Mode B**(控制端监听,agent 穿 NAT 拨出,带重试/退避)。Mode B 已实现为可重连连接器,但尚未被任何已发布二进制使用。
