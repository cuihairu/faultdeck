---
layout: home

hero:
  name: FaultDeck
  text: 面向游戏开发的故障模拟与测试
  tagline: 把延迟、丢包、断线、网络隔离做成可复现、可自动化的测试场景——为游戏开发测试服务。
  actions:
    - theme: brand
      text: 快速开始
      link: /zh/quickstart
    - theme: alt
      text: MVP 计划
      link: /zh/design/mvp-plan
    - theme: alt
      text: English
      link: /

features:
  - icon: 🎮
    title: 为游戏开发测试而生
    details: 场景优先——进房断网、战斗丢包、跨服隔离。游戏场景模板是一等公民。
  - icon: 🧪
    title: 故障 DSL,后端定实现
    details: 实验 YAML 描述故障语义;Toxiproxy / tc netem / WinDivert 决定实现方式。不支持就报「不支持」,绝不静默近似。
  - icon: 🔁
    title: 可靠撤销
    details: 看门狗 + 租约 + 强制回滚——进程被杀故障也一定被撤销。MVP 硬性验收条件。
  - icon: 📄
    title: 可复现的报告
    details: JSON 报告含注入参数、时间线与恢复验证结果。同一 YAML,一致结果。
---

<div style="text-align:center;margin-top:2rem">
<img src="/logo.svg" width="64" height="64" alt="FaultDeck logo" />
</div>

<div style="margin-top:2.5rem">
<img src="/images/architecture-zh.svg" alt="FaultDeck 系统架构：CLI 向协调中枢发送 JSON 指令；各目标主机上的 agent 通过 tc netem 与 iptables 注入故障；报告统一归档并可用 compare_runs 对比" style="width:100%;max-width:1080px;display:block;margin:0 auto" />
</div>
