# Netem 故障

故障模型在 `faultdeck-core`(`FaultSpec`);`tc netem` 适配器在 `faultdeck-netem`;disconnect 路径在 `faultdeck-agent::firewall`。以下内容直接取自枚举与校验代码。

## 支持的故障类型

| 故障 | `FaultSpec` 变体 | 后端 | 参数 | 校验 |
|---|---|---|---|---|
| 延迟 | `Latency` | `tc netem delay` | `milliseconds: u64` | `0 < ms ≤ 86_400_000`(24 小时) |
| 丢包 | `Loss` | `tc netem loss` | `percent: f64` | `0 < percent ≤ 100` |
| 断连 | `Disconnect` | `iptables -t mangle` DROP | `all_traffic: bool`(默认 `true`) | 目标必须 IPv4 |

CLI 选择:`--fault latency|loss|disconnect`。`latency` 必须带 `--milliseconds`,`loss` 必须带 `--percent`,`disconnect` 无参数。

## 目标模型

```rust
pub struct Target {
    pub source: Option<IpNet>,
    pub destination: Option<IpNet>,
    pub destination_port: Option<u16>,
}
```

- CLI 暴露 `--destination <CIDR>`(必填)与 `--port-spec <u16>`(可选的目的端口匹配)。
- 目标必须是 **IPv4** —— 目标中任何 IPv6 网段都会被拒绝(core 报 `UnsupportedIpv6`,firewall 报 `TargetNotV4`)。
- 空目标(source 与 destination 均为空)会被拒绝,报 `EmptyTarget`。

## 安全约束

`inject` 请求到达 agent 后按顺序执行:

1. **管理网段守卫** —— 目标中的每个 CIDR 与 agent 的 `--management` 列表做重叠检查,重叠即拒绝(「管理流量绝不允许被注入故障」)。
2. **内在校验**(`faultdeck_core::validate_injection`)—— 上表中的参数范围与仅 IPv4。
3. **单故障约束** —— agent 只跟踪一个已生效故障;第二个 `inject` 返回 `another fault is already applied; rollback first`。
4. **载荷上限** —— 请求行超过 64 KiB 拒绝。
5. **外部 qdisc preflight** —— 若网卡根 qdisc 不是 FaultDeck 自己的 `100:` 句柄,适配器拒绝运行。

## 适配器生成的 tc 结构

对 `latency`/`loss`,适配器构建 prio 层级,只对匹配波段注入故障:

```
tc qdisc add dev IFACE root handle 100: prio bands 3
tc filter add dev IFACE protocol ip parent 100: prio 1 u32 \
   [match ip src …] [match ip dst …] [match ip dport PORT 0xffff] flowid 100:2
tc qdisc add dev IFACE parent 100:2 handle 50: netem delay 200ms   # 或: loss 30.000
tc qdisc add dev IFACE parent 100:3 fq_codel
tc filter add dev IFACE protocol ip parent 100: prio 2 u32 match u32 0 0 flowid 100:3
```

- 匹配的流量进入波段 `100:2` → netem;其余流量落到 `100:3`(fq_codel),不受影响。
- 百分比固定三位小数格式(`loss 30.000`)。
- 重新应用前会先删除上次遗留的 FaultDeck 根 qdisc。
- 应用过程中任一命令失败,适配器立即回滚根 qdisc。

## Disconnect

`Disconnect` 没有 netem 等价物,因此 agent 将其交给防火墙模块:在 **mangle** 表创建 `FAULTDECK` 链并从 `INPUT` 跳转,对每个目标网段追加一条 `-d ADDR/PREFIX -j DROP` 规则。回滚时对链执行 flush(`-F`)与删除(`-X`)。`--iptables` 必须指向 iptables 二进制,否则 agent 返回 `iptables missing`。

## 回滚

`faultdeck remote rollback` 撤销当前生效的故障:

- netem 路径:删除根 qdisc;`tc` 的「Cannot find / No such device」类错误视为已清理(幂等)。
- firewall 路径:flush 并删除 `FAULTDECK` 链。
- 当前无故障时,agent 应答 `nothing to roll back`。

## Dry-run 模式

`NetemAdapter::dry_run(iface)`(库 API)只校验并返回将要执行的完整 `tc` 命令列表,不触碰系统 —— 测试在用,也适合在注入前检查命令。
