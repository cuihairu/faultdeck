---
layout: home

hero:
  name: FaultDeck
  text: Game-Dev Fault Simulation & Testing
  tagline: Simulate latency, packet loss, disconnects and partitions as reproducible, automated test scenarios — for game development testing.
  actions:
    - theme: brand
      text: Get Started
      link: /quickstart
    - theme: alt
      text: MVP Plan
      link: /design/mvp-plan
    - theme: alt
      text: 中文文档
      link: /zh/

features:
  - icon: 🎮
    title: Built for Game Dev Testing
    details: Scenarios are reproducible today via `--scenario-name` and a stable report hash. Prebuilt game templates (disconnect-on-join, combat packet loss) are on the P2 roadmap.
  - icon: 🧪
    title: Fault DSL, Backend Decides
    details: Fault specs (latency / loss / disconnect) validate before any command runs; the backend answers "unsupported" rather than approximating. The YAML DSL and the Toxiproxy / WinDivert backends are planned.
  - icon: 🔁
    title: Rollback You Can Trust
    details: Rollback is idempotent — tc "not found" errors are tolerated and foreign root qdiscs are never touched; a leftover FaultDeck qdisc is cleaned up on the next apply. Kill-safe forced rollback (watchdog + lease) is planned (FD-P0-4).
  - icon: 📄
    title: Reproducible Reports
    details: JSON reports with injection parameters, timeline, recovery verification and a stable scenario hash for history diff.
---

<div style="text-align:center;margin-top:2rem">
<img src="/logo.svg" width="64" height="64" alt="FaultDeck logo" />
</div>

<div style="margin-top:2.5rem">
<img src="/images/architecture-en.svg" alt="FaultDeck system architecture: CLI sends framed JSON to the coordinator hub; registered agents drive tc netem and iptables faults on target hosts; reports are archived and compared with compare_runs" style="width:100%;max-width:1080px;display:block;margin:0 auto" />
</div>
