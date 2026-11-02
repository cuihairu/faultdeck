# FaultDeck · 轻量级、跨平台、可编排的网络故障实验工具

模拟延迟、丢包、断线、黑洞、带宽限制;按预定义场景自动制造故障、验证应用恢复、生成可复现的测试报告。面向游戏后端、分布式系统、微服务与通用网络应用,不绑定 KBEngine 或单一游戏服务器。

## 核心理念

- **实验 DSL 描述故障语义,Backend 决定实现方式**;Backend 不支持精确语义时返回「不支持」,不做静默近似
- **不重造轮子**:底层引擎复用 Toxiproxy(TCP 代理)/ tc netem(Linux)/ WinDivert(Windows),FaultDeck 做编排、安全与报告
- **可靠撤销是 MVP 硬性验收条件**,不是事后补丁
- **安全隔离**:绝不把故障施加到承载 SSH/ZeroTier 管理流量的网卡

## 架构

```
faultdeck CLI(→ 可选 Web UI)
  └ Rust Experiment Engine
       Scenario Planner / Target Resolver / Safety Guard
       Lifecycle+Rollback / Metrics / Report
       └ Fault Backend Adapters
            ├ Toxiproxy(TCP,三平台)   ├ tc/netem(Linux)
            ├ WinDivert(Windows,P2)    └ Chaos Mesh(K8s,可选)
```

## 路线图

| 阶段 | 内容 | 状态 |
|---|---|---|
| P0 | Rust CLI + Toxiproxy Adapter + 实验 YAML + 延迟/断线/黑洞/带宽 + 自动恢复 + JSON 报告 | 开发中 |
| P1 | tc/netem Adapter、TCP/UDP、流量选择、突发丢包、乱序、netns 隔离 | 待 P0 |
| P2 | 游戏弱网/战斗断线/跨服隔离模板,集成 Wingman API、外部断言与报告 | 待 P1 |
| P3 | WinDivert、可选 Web UI、多目标、远程 Agent、CI 集成 | 待 P2 |

## 仓库结构(目标)

```
crates/ faultdeck-core · scenario · backend · toxiproxy · netem
        safety · report · cli
examples/ tcp-reconnect.yaml · udp-loss.yaml · network-partition.yaml
tests/   integration/ · recovery/
```

## 文档

- [ChatGPT 评审存档](docs/review/2026-11-02-chatgpt-faultdeck.md)
- [MVP 计划](docs/design/mvp-plan.md)
