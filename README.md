# Faultdeck · 分布式编码 Agent 运维平台

把三台机器(编码 VM / home-pve / openclaw-win)上的编码 Agent 会话纳入统一管理:持久化任务、会话接管、失败恢复、随时介入。

源起:2026-11-02 ChatGPT 架构评审([存档](docs/review/2026-11-02-chatgpt-distributed-agent-ops.md))——Hermes Peer 通道不稳定(长任务易丢、无会话接管),评审结论:**先试 Happier(Hermes+Happier 经 MCP 集成,改造最小),后评估 Multica(任务调度中心)**。

## 定位

- **Happier**:分布式 Agent **会话控制**(长会话保持、随时介入、中断续跑)→ 第一优先验证
- **Multica**:多节点**任务系统**(排队、领取、重试、审核)→ 第二阶段评估
- **Faultdeck**:两者的 glue + 本舰队特有的调度规则(席位名册、守卫复活、审计流水线)

## 路线图

| 里程碑 | 内容 | 状态 |
|---|---|---|
| M0 | Happier 调研:安装/登录/`happier mcp serve` 验证 | 进行中(home-pve) |
| M1 | Hermes 集成:`mcp_servers.happier` 四能力验证(创建/查询/中断/发消息) | 待 M0 |
| M2 | 多节点部署:home-pve + openclaw-win,跨机接管实测 | 待 M1 |
| M3 | 任务调度层:批量任务生成/领取/重试/跨节点分配 | 待 M2 |

## 文档

- [ChatGPT 评审存档](docs/review/2026-11-02-chatgpt-distributed-agent-ops.md)
- [设计决策与路线图](docs/design/2026-11-02-faultdeck-m0.md)

## 约定

- 语言/commit:英文短句;禁 force push;测试绿才 push
- 凭据一律 env 引用,不入库
