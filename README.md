<p align="center"><img src="docs/public/logo.svg" width="64" height="64" alt="FaultDeck logo" /></p>

# FaultDeck · Game-Dev-Oriented Fault Simulation & Testing Tool


English · [中文文档](README.zh.md)

**FaultDeck serves game development testing.** It simulates real-world game network faults — player weak networks, disconnect/reconnect, mid-combat drops, cross-server partition — injects them automatically, verifies that the server recovers correctly, and produces reproducible test reports. The goal: repeatable, automated fault testing instead of manually pulling network cables.

Fault types: latency, packet loss, disconnect, blackhole, bandwidth throttle. Scenarios are prebuilt around common game development problems (disconnect-on-room-join and reconnect, combat settlement packet-loss retry, dual-server brain split, Redis/DB timeouts blocking game logic). Not bound to KBEngine or any single game server — but game scenario templates are first-class citizens.

## Core Principles

- **The experiment DSL describes fault semantics; the backend decides implementation.** If a backend cannot honor the exact semantics, it returns "unsupported" — never a silent approximation.
- **No reinvented wheels:** the underlying engines are reused — Toxiproxy (TCP proxy), tc netem (Linux), WinDivert (Windows). FaultDeck owns orchestration, safety, and reporting.
- **Reliable rollback is an MVP acceptance gate**, not an afterthought.
- **Safety isolation:** never apply faults to NICs/subnets carrying management traffic (SSH, ZeroTier).

## Architecture

```
faultdeck CLI (→ optional Web UI)
  └ Rust Experiment Engine
       Scenario Planner / Target Resolver / Safety Guard
       Lifecycle+Rollback / Metrics / Report
       └ Fault Backend Adapters
            ├ Toxiproxy (TCP, 3 platforms)   ├ tc/netem (Linux)
            ├ WinDivert (Windows, P2)        └ Chaos Mesh (K8s, optional)
```

## Roadmap

| Phase | Scope | Status |
|---|---|---|
| P0 | Rust CLI + Toxiproxy adapter + experiment YAML + latency/disconnect/blackhole/bandwidth + auto recovery + JSON report (test-driven: game disconnect-reconnect scenario ships first) | In development |
| P1 | tc/netem adapter, TCP/UDP, traffic selection, burst loss, reordering, netns isolation | Planned |
| P2 | Game weak-network / combat-disconnect / cross-server-partition templates, Wingman API integration, external assertions & reports | Planned |
| P3 | WinDivert, optional Web UI, multi-target, remote agent, CI integration | Planned |

## Repository Layout (target)

```
crates/ faultdeck-core · scenario · backend · toxiproxy · netem
        safety · report · cli
examples/ tcp-reconnect.yaml · lost-packet-retry.yaml · network-partition.yaml
tests/   integration/ · recovery/
```

## Docs

- [MVP plan](docs/design/mvp-plan.md) · [MVP 计划(中文)](docs/design/mvp-plan.zh.md)
- [ChatGPT review archive](docs/review/2026-11-02-chatgpt-faultdeck.md)

## Conventions

- Commits: short English; no force push; tests green before push
- Credentials via env references only, never committed
