# FaultDeck MVP 计划(P0)

日期:2026-11-02 · 源:ChatGPT 架构评审(存档见 docs/review/)+ 用户拍板

## P0 范围:最小可用 TCP 故障实验

- Rust CLI(`faultdeck`)
- Toxiproxy Adapter(TCP 代理故障注入)
- 实验 YAML DSL(场景编排:步骤/持续时间/目标)
- 故障类型:延迟(latency)/断线(cut connection)/黑洞(timeout)/带宽限制(bandwidth)
- 自动恢复(Lifecycle+Rollback,Safety Guard 看门狗:实验中断/进程被杀也必须撤销)
- JSON 实验报告(时间线、注入参数、恢复验证结果)

## 硬性验收

1. `faultdeck run examples/tcp-reconnect.yaml` 全自动跑通,不依赖 GUI
2. **Ctrl-C / kill -9 后故障 100% 被撤销**(watchdog 兜底),目标连接恢复正常
3. 报告 JSON 可复现:同 YAML 重跑结果一致
4. Safety Guard 拒绝对管理网段(SSH/ZeroTier 所在网卡/网段)的实验

## Crate 划分

| crate | 职责 |
|---|---|
| faultdeck-core | 故障类型、目标、状态模型 |
| faultdeck-scenario | 实验 YAML DSL 解析与调度 |
| faultdeck-backend | Backend Trait(语义声明+能力查询) |
| faultdeck-toxiproxy | Toxiproxy REST API 适配 |
| faultdeck-safety | Watchdog、Lease、Rollback |
| faultdeck-report | 指标采集与 JSON 报告 |
| faultdeck-cli | 命令行入口 |

## 里程碑拆分

- [ ] FD-P0-1 仓库骨架:cargo workspace + 7 crate + CI(rustfmt/clippy/test)
- [ ] FD-P0-2 core 类型模型 + scenario YAML 解析(serde,含校验报错)
- [ ] FD-P0-3 toxiproxy adapter(启动/托管 toxiproxy-server,代理与故障 CRUD)
- [ ] FD-P0-4 safety:watchdog 进程 + lease + 强制回滚(含 fork 后父死子活场景)
- [ ] FD-P0-5 CLI run/report 子命令 + JSON 报告
- [ ] FD-P0-6 examples 三个场景 + integration/recovery 测试 + README 快速上手

## P1 预告

tc/netem Adapter(netlink 直操而非 shell 调 tc)、TCP/UDP、流量选择(flow 分流)、突发丢包、乱序、netns 隔离;测试环境用独立 netns,避免污染宿主。

## 跨机控制(2026-11-02 补充:Windows 控制端 → Linux 被测端)

- P0 即可达成:Windows 跑 faultdeck CLI,Linux 被测端跑 toxiproxy-server,经 Toxiproxy REST API 注入延迟/断连/黑洞/限带宽(控制端与被测端仅需网络可达——不需要 SSH、不需要 root、不动网卡)。
- P1 升级:tc/netem 级故障(真丢包/抖动/隔离)——推荐在 Linux 被测端常驻 faultdeck-agent(监听本地端口接收控制端指令,本机执行 netem/iptables,带安全守卫:管理网段白名单,绝不掐管理流量);SSH 代执行作备选。原 P3 的「远程 Agent」形态提前到 P1。

## 连接方向与报告(2026-11-02 补充)

- Agent 双模式:Mode A=agent 监听(目标公网可达);Mode B=控制端监听(目标在 NAT 后,agent 主动出站反向拨号,长连隧道断线自动重连)。单条 TCP 长连接承载指令/事件/日志/报告——两模式协议对称。
- 报告是核心功能:唯一事实源(YAML+hash、注入时间线、恢复证据、断言结果、一键重现),JSON + HTML 双格式,历史对比做回归验证。

- Coordinator 协调者模式(多目标推荐形态):部署在 agent 与控制端都可达处;agent 注册(身份/能力/心跳),控制端附着发起实验;协调者路由指令、存档报告,解决双向 NAT 并提供多目标舰队视图;与直连模式 A/B 递进(单目标直连,多目标走协调者);舰队场景可运行于 meshlet 虚拟网之上。
