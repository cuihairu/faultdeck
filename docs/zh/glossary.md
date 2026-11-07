# 术语表

[English glossary](/glossary)。这些术语按实现职责归纳,不按目录命名。各层落位见[架构](/zh/architecture#源码分层解说)。

| 术语 | English | 定义 | file:line |
|---|---|---|---|
| 故障规格 | `FaultSpec` | 延迟、丢包或断连语义的标签模型,由用户入口、coordinator、agent 和适配器共享。 | `crates/core/src/lib.rs:26` |
| 故障目标 | `Target` | 可选的 IPv4 源地址、目标地址和目标端口选择器,用于限定一次注入。 | `crates/core/src/lib.rs:44` |
| 注入请求 | `Injection` | 一个故障加一个目标;这对数据是共享执行契约。 | `crates/core/src/lib.rs:82` |
| 内在校验 | intrinsic validation | 在后端行为执行前检查故障范围、非空目标和 IPv6 支持状态。 | `crates/core/src/lib.rs:108` |
| 管理网段保护 | management-network protection | 拒绝与操作者提供的受保护 CIDR 重叠的目标。 | `crates/core/src/lib.rs:142` |
| 执行器端口 | `CommandExecutor` | netem 适配器执行或记录 `tc` 命令的边界;生产实现和测试实现都遵守它。 | `crates/netem/src/lib.rs:73` |
| tc 命令 | `TcCommand` | 用程序加参数表示的一次可复现流量控制操作。 | `crates/netem/src/lib.rs:56` |
| netem 适配器 | `NetemAdapter` | 在单网卡上规划、应用并回滚延迟与丢包的 qdisc/filter 状态。 | `crates/netem/src/lib.rs:172` |
| pilot qdisc | pilot qdisc | FaultDeck 拥有的根 qdisc 句柄 `100:`,用于识别并只安全删除自己的流量控制状态。 | `crates/netem/src/lib.rs:13` |
| 断连路由 | disconnect routing | Agent 把 disconnect 注入路由到 iptables,其余故障路由到 netem。 | `crates/agent/src/lib.rs:196` |
| iptables 链 | iptables chain | `FAULTDECK` mangle 链及其 `INPUT` 跳转承载断连 DROP 规则。 | `crates/agent/src/firewall.rs:9` |
| 断连适配器 | disconnect adapter | `Iptables` 确保链存在、安装 DROP 规则,并执行幂等回滚。 | `crates/agent/src/firewall.rs:31` |
| Agent 配置 | `AgentConfig` | 进程启动配置:监听地址、可选 token、目标网卡和管理网段。 | `crates/agent/src/lib.rs:90` |
| 单活动故障 | single active fault | Agent 宿主状态;回滚清除适配器标记前拒绝第二次注入。 | `crates/agent/src/lib.rs:191` |
| Agent 线请求 | agent wire request | Agent 宿主解码的 `ping`、`inject` 或 `rollback` 标签载荷。 | `crates/agent/src/lib.rs:109` |
| Agent 线响应 | agent wire response | 返回给 CLI 的 `ok` 或 `error` 标签结果。 | `crates/agent/src/lib.rs:117` |
| 令牌鉴权 | token authentication | 每个进程边界的可选共享密钥比较;未设置 token 表示关闭鉴权。 | `crates/agent/src/lib.rs:124` |
| NDJSON 帧 | NDJSON frame | 以换行分界的 JSON 值,硬性上限 64 KiB。 | `crates/agent/src/transport.rs:56` |
| Mode A | Mode A | Agent 监听拓扑:控制器向常驻目标 Agent 打开一条 TCP 流。 | `crates/agent/src/transport.rs:94` |
| Mode B | Mode B | 控制器监听的反向连接拓扑,通过重试和退避穿越 NAT。 | `crates/agent/src/transport.rs:118` |
| 中枢信封 | `WireMessage` | coordinator 信封,携带可选 token 和一条控制命令。 | `crates/coordinator/src/lib.rs:235` |
| 中枢命令 | `AgentCommand` | coordinator 接受的注册、心跳、注入、回滚、取报告或列 Agent 操作。 | `crates/coordinator/src/lib.rs:69` |
| 中枢响应 | `AgentResponse` | coordinator 的确认、入队回执、注册表列表、报告或错误结果。 | `crates/coordinator/src/lib.rs:80` |
| Agent 身份 | `AgentIdentity` | coordinator 保存的 Agent ID、能力列表、最近心跳时间和列表时计算的在线标志。 | `crates/coordinator/src/lib.rs:51` |
| 心跳 | heartbeat | 更新某个已注册 Agent 的 `last_seen_ms`;新鲜度参与在线判定。 | `crates/coordinator/src/lib.rs:178` |
| Agent 链路 | agent link | agent 到 coordinator 的出站连接;`LinkMessage::Dispatch` 沿其下行,`Result`/`Heartbeat` 沿其上行。 | `crates/coordinator/src/lib.rs:134` |
| 在线状态 | online status | 链路存活或心跳在 `HEARTBEAT_TIMEOUT_MS`(30 s)内;离线 agent 从 `available` 剔除。 | `crates/coordinator/src/lib.rs:211` |
| 指令入队 | queued command | 目标 agent 离线时,inject/rollback 持久保存在其队列中,下次注册时补发。 | `crates/coordinator/src/lib.rs:351` |
| 场景 | `Scenario` | 带名称的注入;其稳定序列化是哈希和报告的输入。 | `crates/report/src/lib.rs:38` |
| 场景哈希 | scenario hash | 场景的 SHA-256 身份,用于报告查找和比较。 | `crates/report/src/lib.rs:49` |
| 运行报告 | `RunReport` | 权威结果模型,包含场景身份、时间戳、证据和回放文本。 | `crates/report/src/lib.rs:78` |
| 回放命令 | replay command | 用户入口生成、保存进报告的 shell 引号安全 CLI 命令。 | `crates/cli/src/main.rs:315` |
| 恢复证据 | restore evidence | 回滚后的带时间戳检查,包含通过/失败和详情字段。 | `crates/report/src/lib.rs:63` |
| 外部断言 | external assertion | 附加到运行报告的具名外部观察结果。 | `crates/report/src/lib.rs:71` |
| 历史 diff | historical diff | 对两份报告的事件、证据和断言集合做结构比较。 | `crates/report/src/lib.rs:175` |
