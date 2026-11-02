
## 跨机控制(2026-11-02 补充,用户需求:Windows 控制端 → Linux 被测端)

- P0 即可达成:Windows 跑 faultdeck CLI,Linux 跑 toxiproxy-server,经 REST API 注入延迟/断连/黑洞/限带宽(控制端与被控端仅需网络可达,不需 SSH/root/动网卡)。
- P1 升级:tc/netem 级故障(真丢包/抖动/隔离)——推荐 faultdeck-agent 常驻 Linux 端(监听本地端口,接收控制端指令,本机执行 netem/iptables,带安全守卫:管理网段白名单,绝不掐管理流量);SSH 代执行作备选。原 P3 的「远程 Agent」形态提前到 P1。

## 连接方向与报告(2026-11-02 补充)

- agent 双模式:Mode A=agent 监听(目标公网可达);Mode B=控制端监听(agent 反向拨号,长连隧道)。单条 TCP 长连接承载指令/事件/日志/报告,协议对称。
- 报告是关键功能:唯一事实源(YAML+hash/注入时间线/恢复证据/断言结果/重现命令),JSON+HTML 双格式,支持历史对比回归。
