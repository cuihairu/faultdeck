# Quickstart

Get FaultDeck running and inject your first network fault. Everything here is verified against the current implementation (`faultdeck remote inject` → resident agent → `tc netem` / `iptables`).

## Prerequisites

- **Rust toolchain** (stable) — FaultDeck builds from source; no binary releases yet.
- **Linux** for fault injection — the `tc` adapter refuses to run on non-Linux hosts (a `dry_run` mode exists for testing elsewhere).
- `tc` from **iproute2**, and `iptables` if you plan to use the `disconnect` fault.
- Root or `CAP_NET_ADMIN` on the agent host — `tc qdisc` and `iptables -t mangle` both require it.

## 1. Build

```bash
git clone https://github.com/cuihairu/faultdeck.git
cd faultdeck
cargo build --release
```

This produces two binaries:

| binary | role |
|---|---|
| `faultdeck-agent` | resident daemon on the target host; applies faults |
| `faultdeck` | CLI you run locally; talks to the agent over TCP |

## 2. Start an agent

```bash
sudo ./target/release/faultdeck-agent \
  --bind 127.0.0.1:7000 \
  --interface eth0 \
  --token s3cret \
  --management 10.0.0.0/8 --management 192.168.1.0/24 \
  --iptables /usr/sbin/iptables
```

Flags (all optional except shown defaults):

- `--bind` — listen address, default `127.0.0.1:7000`
- `--interface` — NIC that `tc` operates on, default `eth0`
- `--token` — shared secret; when set, every request must carry it
- `--management` — repeatable CIDR that faults must never overlap (the agent warns loudly if you configure none)
- `--iptables` — path to the `iptables` binary; **required for the `disconnect` fault**

## 3. Ping, inject, roll back

```bash
# connectivity check
./target/release/faultdeck remote ping --host 127.0.0.1 --port 7000 --token s3cret
# → pong

# add 200ms delay to traffic destined for 192.0.2.0/24 port 7000
./target/release/faultdeck remote inject \
  --host 127.0.0.1 --port 7000 --token s3cret \
  --fault latency --milliseconds 200 \
  --destination 192.0.2.0/24 --port-spec 7000

# packet loss
./target/release/faultdeck remote inject \
  --host 127.0.0.1 --port 7000 --token s3cret \
  --fault loss --percent 30 \
  --destination 192.0.2.0/24

# undo the fault
./target/release/faultdeck remote rollback \
  --host 127.0.0.1 --port 7000 --token s3cret
```

Rules to know before your first run:

- **One fault at a time per agent connection lifecycle.** A second `inject` is rejected with `another fault is already applied; rollback first`.
- The agent refuses to overwrite a **foreign root qdisc** on the interface (anything other than FaultDeck's own `100:` handle).
- Targets are **IPv4 CIDRs only**; IPv6 targets are rejected.

## 4. Produce a report

Add report flags to `inject` — the CLI writes the report locally after a successful injection:

```bash
./target/release/faultdeck remote inject \
  --host 127.0.0.1 --token s3cret \
  --fault loss --percent 50 --destination 198.51.100.0/24 \
  --scenario-name combat-loss \
  --report-json run1.json --report-html run1.html
```

See [Reports](/reports) for the report format and history diff.

## Next steps

- [Architecture](/architecture) — how the 6 crates fit together
- [Netem Faults](/netem-faults) — fault types, validation bounds, tc layout
- [Agents](/remote-agents) — deployment, auth, and the coordinator protocol
