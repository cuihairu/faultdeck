# FaultDeck MVP Plan (P0)

Date: 2026-11-02 · Source: ChatGPT architecture review (archive in docs/review/) + user decision
中文版:[中文文档](/zh/design/mvp-plan)

## P0 Scope: minimal viable TCP fault experiments

- Rust CLI (`faultdeck`)
- Toxiproxy adapter (TCP proxy fault injection)
- Experiment YAML DSL (scenario orchestration: steps / duration / targets)
- Fault types: latency, cut connection, blackhole (timeout), bandwidth throttle
- Automatic recovery (Lifecycle + Rollback, Safety Guard watchdog: faults must be undone even if the process is killed)
- JSON experiment report (timeline, injection parameters, recovery verification result)

## Hard acceptance gates

1. `faultdeck run examples/tcp-reconnect.yaml` runs fully automated, no GUI
2. **After Ctrl-C / kill -9, 100% of faults are rolled back** (watchdog fallback) and target connections return to normal
3. Reports are reproducible: re-running the same YAML yields consistent results
4. Safety Guard refuses experiments targeting management networks (NICs/subnets carrying SSH/ZeroTier)

## Crate layout

| crate | responsibility |
|---|---|
| faultdeck-core | fault types, targets, state model |
| faultdeck-scenario | experiment YAML DSL parsing & scheduling |
| faultdeck-backend | backend trait (semantic declaration + capability query) |
| faultdeck-toxiproxy | Toxiproxy REST API adapter |
| faultdeck-safety | watchdog, lease, forced rollback |
| faultdeck-report | metrics collection & JSON report |
| faultdeck-cli | command-line entry |

## Milestones

- [ ] FD-P0-1 repo skeleton: cargo workspace + 7 crates + CI (rustfmt/clippy/test)
- [ ] FD-P0-2 core type model + scenario YAML parsing (serde, with validation errors)
- [ ] FD-P0-3 toxiproxy adapter (hosted toxiproxy-server, proxy & fault CRUD)
- [ ] FD-P0-4 safety: watchdog process + lease + forced rollback (incl. orphaned-child scenario)
- [ ] FD-P0-5 CLI run/report subcommands + JSON report
- [ ] FD-P0-6 three example scenarios + integration/recovery tests + README quick start

## P1 preview

tc/netem adapter (netlink directly, not shelling out to tc), TCP/UDP, traffic selection, burst loss, reordering, netns isolation; test environments use dedicated netns to keep the host clean.

## Cross-machine control (added 2026-11-02: Windows control box → Linux target)

- Available in P0: run the faultdeck CLI on Windows, run toxiproxy-server on the Linux target, inject latency/cut/blackhole/bandwidth faults over Toxiproxy's REST API (control box and target only need network reachability — no SSH, no root, no NIC manipulation).
- P1 upgrade: tc/netem-class faults (real packet loss / jitter / partition) — recommended: a resident faultdeck-agent on the Linux target (local port listens for commands from the control box, applies netem/iptables locally, with safety guard: management subnet allowlist, never kills management traffic); SSH exec as fallback. The former P3 "remote agent" pulls forward into P1.
