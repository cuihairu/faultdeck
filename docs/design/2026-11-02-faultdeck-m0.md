# Faultdeck M0 · Happier 调研与验证

日期:2026-11-02 · 决策人:用户令(源 ChatGPT 评审,已代决拍板)

## 决策

1. **不再修补 Hermes Peer 通道**(peer run 长任务易丢、无会话接管、串行队列)。
2. **M0-M1:验证 Happier**——Hermes 主控不变,编码会话(Claude Code/Codex)交给 Happier 管理,经 MCP(`happier mcp serve`)对接。
3. **M3:评估 Multica**——若需要「todo 批量生成任务+数十 worker 领取+重试」,再引入;许可证是自定义条款,商用前审。

## M0 验收清单(home-pve 先行)

- [ ] Happier 安装(方式待考:npm/cargo/binary),`happier --version` 可用
- [ ] 登录/认证打通
- [ ] `happier mcp serve` 起服务,列出工具清单(应有:创建会话/查询/中断/发消息)
- [ ] PVE Hermes `~/.hermes/config.yaml` 挂 `mcp_servers.happier`,四能力各实测一次并留证据
- [ ] 长会话测试:启动一个 claude 会话做小任务 → 中途手机/外部介入发消息 → 观察接管与续跑
- [ ] 结论落 docs/review/m0-happier-poc.md:可行/不可行 + 卡点清单

## M1 预埋(本机编码 VM)

- 本机 Hermes 同样挂 happier MCP(若 M0 通过)
- 迁移试点:把 meshlet 监控的「派单走 peer」路径换成 happier 会话

## 风险

- Happier 与本舰队 tmux 席位模型的关系待定义(共存 or 替换)
- Windows 节点(openclaw)是否支持 Happier daemon 未验证
- ChatGPT 评审本身未端到端实测,所有结论以 M0 实测为准
