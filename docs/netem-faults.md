# Netem Faults

The fault model lives in `faultdeck-core` (`FaultSpec`); the `tc netem` adapter lives in `faultdeck-netem`, and the disconnect path lives in `faultdeck-agent::firewall`. Everything below is taken directly from the enums and validation code.

## Supported fault types

| fault | `FaultSpec` variant | backend | parameter | validation |
|---|---|---|---|---|
| latency | `Latency` | `tc netem delay` | `milliseconds: u64` | `0 < ms ≤ 86_400_000` (24 h) |
| loss | `Loss` | `tc netem loss` | `percent: f64` | `0 < percent ≤ 100` |
| disconnect | `Disconnect` | `iptables -t mangle` DROP | `all_traffic: bool` (defaults to `true`) | target must be IPv4 |

CLI selection: `--fault latency|loss|disconnect`. `latency` requires `--milliseconds`, `loss` requires `--percent`, `disconnect` takes no parameter.

## Target model

```rust
pub struct Target {
    pub source: Option<IpNet>,
    pub destination: Option<IpNet>,
    pub destination_port: Option<u16>,
}
```

- The CLI exposes `--destination <CIDR>` (required) and `--port-spec <u16>` (optional destination-port match).
- Targets must be **IPv4** — any IPv6 network in the target is rejected with `UnsupportedIpv6` (core) / `TargetNotV4` (firewall).
- An empty target (no source and no destination) is rejected with `EmptyTarget`.

## Safety constraints

Enforced in order when an `inject` request reaches the agent:

1. **Management-network guard** — every CIDR in the target is checked against the agent's `--management` list; any overlap rejects the request ("management traffic must never be faulted").
2. **Intrinsic validation** (`faultdeck_core::validate_injection`) — parameter ranges and IPv4-only, as tabled above.
3. **Single active fault** — the agent tracks one applied fault; a second `inject` returns `another fault is already applied; rollback first`.
4. **Payload cap** — request line larger than 64 KiB is rejected.
5. **Foreign-qdisc preflight** — the adapter refuses to run if the interface's root qdisc is not FaultDeck's own `100:` handle.

## tc layout produced by the adapter

For `latency`/`loss` the adapter builds a prio hierarchy that faults only the matched band:

```
tc qdisc add dev IFACE root handle 100: prio bands 3
tc filter add dev IFACE protocol ip parent 100: prio 1 u32 \
   [match ip src …] [match ip dst …] [match ip dport PORT 0xffff] flowid 100:2
tc qdisc add dev IFACE parent 100:2 handle 50: netem delay 200ms   # or: loss 30.000
tc qdisc add dev IFACE parent 100:3 fq_codel
tc filter add dev IFACE protocol ip parent 100: prio 2 u32 match u32 0 0 flowid 100:3
```

- Matched traffic lands in band `100:2` → netem; everything else falls through to `100:3` (fq_codel) untouched.
- Percent is formatted with three decimals (`loss 30.000`).
- A leftover FaultDeck root qdisc from a previous run is deleted before re-applying.
- If any command fails during apply, the adapter immediately rolls back the root qdisc.

## Disconnect

`Disconnect` has no netem equivalent, so the agent routes it to the firewall module: an `iptables` chain `FAULTDECK` in table **mangle**, jumped from `INPUT`. Each target network gets a DROP rule — destination networks match with `-d`, a source network (via the library API) with `-s`. When a destination port is set, port-level disconnect drops both TCP and UDP on that port (`-p tcp --dport P` and `-p udp --dport P`); otherwise all traffic to the target is dropped. A partial install failure tears the chain down before returning the error. Rollback removes the INPUT jump, then flushes (`-F`) and deletes (`-X`) the chain — idempotent when the chain is already gone. The `--iptables` flag must point at the binary, otherwise the agent returns `iptables missing`.

## Rollback

`faultdeck remote rollback` removes the applied fault:

- netem path: deletes the root qdisc; `tc` "Cannot find / No such device" style errors are treated as already-clean (idempotent).
- firewall path: flush + delete the `FAULTDECK` chain.
- With nothing applied, the agent answers `nothing to roll back`.

## Dry-run mode

`NetemAdapter::dry_run(iface)` (library API) validates and returns the exact `tc` command list without touching the system — used by tests and useful for inspecting what a fault would execute.
