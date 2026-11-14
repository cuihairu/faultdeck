# 快速开始

让 FaultDeck 跑起来并注入第一个网络故障。以下内容全部对照当前实现取证（`faultdeck remote inject` → 常驻 agent → `tc netem` / `iptables`）。

## 前置条件

- **Rust 工具链**（stable）—— FaultDeck 目前从源码构建，暂无二进制发布。
- **Linux** 用于故障注入 —— `tc` 适配器在非 Linux 主机上直接报错（另有 `dry_run` 模式可供测试）。
- iproute2 的 `tc`；如需 `disconnect` 故障还需 `iptables`。
- agent 主机上需要 root 或 `CAP_NET_ADMIN` —— `tc qdisc` 和 `iptables -t mangle` 都需要。

## 1. 构建

```bash
git clone https://github.com/cuihairu/faultdeck.git
cd faultdeck
cargo build --release
```

产出五个二进制:

| 二进制 | 角色 |
|---|---|
| `faultdeck-agent` | 目标主机上的常驻守护进程,负责实际施加故障 |
| `faultdeck` | 本地 CLI,通过 TCP 与 agent 通信 |
| `faultdeck-coordinator` | 可选的多目标中枢;维护 agent 注册表与报告归档,经 agent 链路在线下发指令 |
| `faultdeck-mcp` | 可选的 MCP server(stdio),把同一条 hub 链路暴露给 AI agent |
| `faultdeck-desktop` | 可选的 Tauri 桌面 UI;源码构建需要平台对应的 Tauri 系统库。目前尚未发布预编译安装器——nightly 构建排除了该 crate(`.github/workflows/nightly.yml` 中的 `--exclude faultdeck-desktop`) |

## 2. 启动 agent

```bash
sudo ./target/release/faultdeck-agent \
  --bind 127.0.0.1:7000 \
  --interface eth0 \
  --token s3cret \
  --management 10.0.0.0/8 --management 192.168.1.0/24 \
  --iptables /usr/sbin/iptables
```

参数(未标注即有默认值):

- `--bind` —— 监听地址,默认 `127.0.0.1:7000`
- `--interface` —— `tc` 操作的网卡,默认 `eth0`
- `--token` —— 共享密钥;设置后每个请求都必须携带
- `--management` —— 可重复的保护网段 CIDR,故障目标绝不允许与之重叠(未配置时 agent 会大声警告)
- `--iptables` —— 可选的路径覆盖;默认取 PATH 上的 `iptables`,因此装有它的任何主机开箱即可用 `disconnect`

## 3. Ping、注入、回滚

```bash
# 连通性检查
./target/release/faultdeck remote ping --host 127.0.0.1 --port 7000 --token s3cret
# → pong

# 给发往 192.0.2.0/24 端口 7000 的流量加 200ms 延迟
./target/release/faultdeck remote inject \
  --host 127.0.0.1 --port 7000 --token s3cret \
  --fault latency --milliseconds 200 \
  --destination 192.0.2.0/24 --port-spec 7000

# 丢包
./target/release/faultdeck remote inject \
  --host 127.0.0.1 --port 7000 --token s3cret \
  --fault loss --percent 30 \
  --destination 192.0.2.0/24

# 撤销故障
./target/release/faultdeck remote rollback \
  --host 127.0.0.1 --port 7000 --token s3cret
```

第一次运行前必须知道的规则:

- **同一时刻一个 agent 只有一个已生效故障。** 生效状态跨连接共享(任一连接的 rollback 都能清除);第二次 `inject` 会被拒绝:`another fault is already applied; rollback first`。agent 重启后不保留该状态。
- agent 拒绝覆盖网卡上**非 FaultDeck 的根 qdisc**(根句柄不是自己的 `100:`)。
- 目标只支持 **IPv4 CIDR**;IPv6 目标会被拒绝。

## 4. 产出报告

给 `inject` 加报告参数 —— 注入成功后由 CLI 在本地写报告:

```bash
./target/release/faultdeck remote inject \
  --host 127.0.0.1 --token s3cret \
  --fault loss --percent 50 --destination 198.51.100.0/24 \
  --scenario-name combat-loss \
  --report-json run1.json --report-html run1.html
```

报告格式与历史 diff 见[报告](/zh/reports)。

## 下一步

- [架构](/zh/architecture) —— 8 个 crate 如何协作
- [Netem 故障](/zh/netem-faults) —— 故障类型、校验边界、tc 结构
- [Agent 与注册](/zh/remote-agents) —— 部署、鉴权与 coordinator 协议
