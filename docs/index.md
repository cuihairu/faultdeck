---
layout: home

hero:
  name: FaultDeck
  text: Game-Dev Fault Simulation & Testing
  tagline: Simulate latency, packet loss, disconnects and partitions as reproducible, automated test scenarios — for game development testing.
  actions:
    - theme: brand
      text: MVP Plan
      link: /design/mvp-plan
    - theme: alt
      text: 中文文档
      link: /zh/

features:
  - icon: 🎮
    title: Built for Game Dev Testing
    details: Scenarios first — disconnect-on-join, combat packet loss, cross-server partition. Game templates are first-class citizens.
  - icon: 🧪
    title: Fault DSL, Backend Decides
    details: Experiment YAML describes fault semantics; Toxiproxy / tc netem / WinDivert decide implementation. "Unsupported" beats silent approximation.
  - icon: 🔁
    title: Reliable Rollback
    details: Watchdog + lease + forced rollback — faults are undone even if the process is killed. The MVP hard acceptance gate.
  - icon: 📄
    title: Reproducible Reports
    details: JSON reports with injection parameters, timeline and recovery verification. Same YAML, consistent results.
---

<div style="text-align:center;margin-top:2rem">
<img src="/logo.svg" width="64" height="64" alt="FaultDeck logo" />
</div>
