# 配置

FaultDeck 没有配置文件 —— 一切都在进程启动时以 CLI 参数传入。下表默认值直接取自源码。

## faultdeck-agent

| 参数 | 默认 | 说明 |
|---|---|---|
| `--bind ADDR` | `127.0.0.1:7000` | TCP 监听地址 |
| `--token TOKEN` | 不设 → 鉴权关闭 | 设置后每个请求行必须以 `<token> ` 开头 |
| `--interface IFACE` | `eth0` | `tc` 适配器操作的网卡 |
| `--management CIDR` | 空 | 可重复;保护网段,重叠即拒绝注入 |
| `--iptables PATH` | 无 | `disconnect` 必需;缺失时 agent 应答 `iptables missing` |

编译期常量(不可配置):

| 常量 | 值 | 位置 |
|---|---|---|
| 请求载荷上限 | 64 KiB | `faultdeck_agent::MAX_INJECTION_BYTES` |
| 引导 qdisc 句柄 | `100:` | `faultdeck_netem::PILOT_QDISC_HANDLE` |
| netem qdisc 句柄 | `50:` | 适配器内部 |
| 最大延迟 | 86 400 000 ms(24 小时) | `faultdeck_core::MAX_LATENCY_MS` |
| iptables 链 | `FAULTDECK`,表 `mangle` | `faultdeck_agent::firewall::CHAIN` |

`AgentConfig`(参数背后的库结构体)支持 JSON 序列化,可编程构造。

## faultdeck-coordinator

```bash
faultdeck-coordinator --bind 127.0.0.1:7001 [--token <shared-secret>]
```

`--bind` 默认为 `127.0.0.1:7001`。`--token` 可选;设置后每条线上消息都必须携带匹配的 token(见 `remote-agents.md`)。状态(注册表、报告归档、路由)均在内存。

## faultdeck(CLI)

| 参数 | 适用 | 默认 | 说明 |
|---|---|---|---|
| `--host` | 所有 `remote` 子命令 | 必填 | agent 地址 |
| `--port` | 所有 `remote` 子命令 | `7000` | agent 端口 |
| `--token` | 所有 `remote` 子命令 | 无 | 前置到请求行 |
| `--fault` | `inject` | 必填 | `latency` \| `loss` \| `disconnect` |
| `--destination` | `inject` | 必填 | IPv4 CIDR |
| `--port-spec` | `inject` | 无 | 可选目的端口匹配 |
| `--milliseconds` | `inject --fault latency` | 该故障必需 | 延迟毫秒 |
| `--percent` | `inject --fault loss` | 该故障必需 | 丢包百分比 |
| `--scenario-name` | `inject` | `"scenario"` | 报告标签 |
| `--report-json` | `inject` | 无 | 本地写 JSON 报告 |
| `--report-html` | `inject` | 无 | 本地写 HTML 报告 |

没有环境变量、没有配置文件查找,守护进程也没有上述参数之外的选项。
