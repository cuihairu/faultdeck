
## 跨机控制(2026-11-02 补充,用户需求:Windows 控制端 → Linux 被测端)

- P0 即可达成:Windows 跑 faultdeck CLI,Linux 跑 toxiproxy-server,经 REST API 注入延迟/断连/黑洞/限带宽(控制端与被控端仅需网络可达,不需 SSH/root/动网卡)。
- P1 升级:tc/netem 级故障(真丢包/抖动/隔离)——推荐 faultdeck-agent 常驻 Linux 端(监听本地端口,接收控制端指令,本机执行 netem/iptables,带安全守卫:管理网段白名单,绝不掐管理流量);SSH 代执行作备选。原 P3 的「远程 Agent」形态提前到 P1。
