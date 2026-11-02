

===== 片段 0 (1265 字) =====

我觉得 **FaultDeck 比单纯的 GameBot 更值得独立开发**，尤其适合你现在的游戏后端项目生态。

但我想把最初的想法升级一下：

\u003e **FaultDeck：一个轻量级、跨平台、可编排的网络故障实验工具。**
\u003e
\u003e 不仅模拟延迟、丢包和断线，还能按照预定义场景自动制造故障、验证应用能否恢复，并生成可复现的测试报告。

它应该同时面向游戏开发、分布式系统、微服务和普通网络应用，而不是绑定 KBEngine 或某一个游戏服务器。

## 一、FaultDeck 到底能解决什么问题？

假设你的游戏服务器出现以下情况：

- 玩家进入房间后突然断网，重连时发现角色还停留在旧房间。
- 战斗结算响应丢失，客户端重试后奖励发放两次。
- 两台服务器之间的网络断开，双方都认为自己仍然拥有房间管理权。
- 网络抖动造成玩家操作延迟、位置回滚、状态不同步。
- Redis 或数据库连接突然超时，游戏逻辑被阻塞。
- 主播与游戏服务器短暂失联，恢复后房间里的其他参与者状态异常。

这些问题通过正常的单元测试很难完整复现。

FaultDeck 的目的就是让你能够指定：

**什么时候发生故障 → 影响哪些连接 → 持续多久 → 怎样恢复 → 系统恢复后应该满足什么条件。**

## 二、它可以模拟哪些故障？

| 故障 | 具体效果 | 游戏场景 |
|--------|--------|--------|
| Latency | 固定延迟、随机抖动 | 高延迟玩家操作 |
| Packet Loss | 随机丢包、连续突发丢包 | Wi-Fi/移动网络不稳定 |
| Reorder | 网络包乱序 | UDP/KCP 帧同步 |
| Duplicate | 网络包重复 | UDP 消息去重 |
| Corruption | 修改少量数据位 | 协议异常处理 |
| Bandwidth | 限速、突发带宽下降 | 弱网资源下载 |
| Disconnect | TCP RST、连接关闭 | 强制断线 |
| Blackhole | 静默丢弃流量 | 半开连接、心跳超时 |
| Partition | 指定节点间网络隔离 | 跨服通信、脑裂 |
| DNS Failure | DNS 解析故障 | 依赖服务连接 |
| Flapping | 网络反复断开恢复 | 频繁重连 |
| Asymmetric | 上行正常、下行异常 | 单向网络故障 |

有一个重要的技术区别：

**TCP 丢包不等于游戏业务消息丢失。** TCP 通常会重传并保持字节顺序，所以 TCP 场景中更多表现为延迟、吞吐量下降和超时；而 UDP/KCP 会直接受到丢包、乱序、重复报文影响。

因此 FaultDeck 必须区分网络层故障和应用层故障，不能简单地把两者混在一起。

## 三、现成开源工具有哪些？还值得自己做吗？

我查了几种成熟方案。

image_group{\\

===== 片段 1 (5854 字) =====

]**

Windows · WinDivert · TCP/UDP

可以在不修改目标程序的情况下，通过 WinDivert 实现延迟、丢包、乱序和数据篡改。citeturn749594search0

局限：主要是本机交互式工具，不是现成的跨平台分布式实验编排系统。

urlGitHub 项目https://github.com/jagt/clumsy

**所以，我并不建议 FaultDeck 重新发明一个类似 netem、Toxiproxy 或 clumsy 的底层网络引擎。**

真正值得开发的，是它们上面缺少的一层：

**统一的故障描述、跨平台执行适配、游戏场景模板、自动恢复和结果验证。**

## 四、我最想做的功能：故障场景编排

单纯提供一个延迟滑块，或者输入 `loss 10%`，意义有限。

真正有价值的是能描述一个完整的实验。

### 示例：玩家匹配时突然断网

实验：匹配成功后断网 12 秒

以下为设计示例，不是现有 FaultDeck 的执行结果。

0–10s

正常

10–22s

断网

22–30s

恢复

30–45s

验证

45s

结束

---

**建立基线**

虚拟玩家登录、开始匹配、记录连接和房间状态

**注入故障**

仅对目标游戏连接施加 12 秒双向黑洞

**恢复网络**

自动撤销故障，观察心跳、重连和会话恢复

**执行断言**

验证成功重连、没有重复角色、房间所有权一致、玩家状态未损坏

这个实验要支持自动运行，可以设计一份配置：

```yaml
apiVersion: faultdeck.dev/v1alpha1
kind: Experiment

metadata:
  name: game-reconnect-test

target:
  name: game-gateway
  driver: toxiproxy
  protocol: tcp
  direction: both

safety:
  environment: staging
  maxDuration: 60s
  autoRollback: true

steps:
  - at: 0s
    action: baseline

  - at: 10s
    action: blackhole
    duration: 12s

  - at: 22s
    action: recover

  - at: 30s
    action: verify
    checks:
      - player_reconnected
      - room_state_consistent
      - no_duplicate_session
```

这是我建议的 DSL 草案，真正执行前还需要配置目标代理地址和对应的业务验证接口。

其中 `player_reconnected` 等业务断言不能由纯网络引擎凭空推断，需要 Wingman、测试客户端或游戏服务提供相应的观测信息。

### 更值得预置的游戏故障场景

| 场景模板 | 故障方式 | 重点验证 |
|--------|--------|--------|
| 玩家弱网 | 高延迟 + 抖动 + 丢包 | 移动同步、客户端预测 |
| 登录断线 | 连接建立期间断开 | 登录状态和会话清理 |
| 战斗断线 | 战斗中网络黑洞 | 重连、快照恢复 |
| 帧同步异常 | UDP 乱序 + 丢包 | 帧序号、补帧、回滚 |
| 跨服隔离 | GameServer 与协调节点断联 | 房间所有权、状态一致性 |
| 数据库超时 | DB 连接延迟或无响应 | 降级、超时、数据可靠性 |
| 支付响应丢失 | 回调链路中断 | 重试与发货幂等 |
| 主播网络抖动 | 网络反复中断 | 主播状态恢复、房间清理 |

这里要把网络实验和业务故障实验区分开。例如“HTTP 请求重试后重复发奖”属于业务幂等验证，不能仅凭制造重复 TCP 数据包来模拟。FaultDeck 只负责故障，业务测试由应用侧适配器提供。

---

## 五、我建议的技术架构

你喜欢 Rust，又有 Windows、Linux、多台服务器的环境。我认为 Rust 非常适合实现 FaultDeck 的控制核心，但没必要用 Rust 重写 Linux 内核已经拥有的网络模拟机制。

**FaultDeck CLI / 可选 Web UI**

实验创建、模板选择、执行记录、报告查看

**Rust Experiment Engine**

Scenario Planner

Target Resolver

Safety Guard

Lifecycle / Rollback

Metrics Collector

Report Generator

Fault Backend Adapters

**TCP Proxy**

Toxiproxy

**Linux**

tc/netem、Chaosd

**Windows**

WinDivert

**macOS**

代理模式优先

**被测应用与网络**

游戏客户端 / Gateway / GameServer / Redis / 数据库

### 为什么一定要做 Backend Adapter？

因为不同操作系统、不同网络协议适合的故障注入方式不同。

| 平台/模式 | 推荐技术 | 第一版 |
|--------|--------|--------|
| Windows / Linux / macOS TCP | Toxiproxy | (支持) |
| Linux TCP / UDP | tc netem、可通过 Chaosd 调用 | (支持) |
| Docker 容器 | 容器网络命名空间 + netem | 第二阶段 |
| Windows TCP / UDP | WinDivert 原生适配器 | 第二阶段 |
| macOS TCP | Toxiproxy | (支持) |
| macOS 原生全局网络模拟 | Network Link Conditioner 等系统能力 | 后续评估 |
| Kubernetes | Chaos Mesh | 可选适配 |

Windows 可以参考 WinDivert 的捕获、过滤和重注入能力，但运行时需要相应的驱动权限。macOS 的 Network Link Conditioner 更适合本机开发调试，不应该假设它提供了与 Linux tc 完全相同的细粒度自动化控制接口。citeturn732104search7turn732104search0

一个很重要的实现原则是：**实验 DSL 描述“故障语义”，Backend 决定“怎样实现”。**

例如 `loss: 10%`：

- Linux netem：真正的网络包丢失。
- Toxiproxy：丢失代理处理的数据块，语义不等同于 IP 包丢失。
- Windows WinDivert：网络包级丢弃。

如果某个 Backend 不支持要求的精确语义，就应该返回“不支持”，而不是悄悄做一个近似实现。

## 六、和你现有项目怎样配合？

我不建议把 FaultDeck 直接合并进 Wingman 或 Cockpit。

这三个产品的职责不同。

| 项目 | 应该负责什么 |
|--------|--------|
| **FaultDeck** | 故障注入、实验执行、恢复、故障报告 |
| urlWingmanhttps://github.com/cuihairu/wingman | 自动化玩家行为、启动测试工作流 |
| urlCockpithttps://github.com/cuihairu/cockpit | 提供基础设施节点信息和监控入口 |
| Kode | 调试 KBEngine 开发环境、分析协议 |
| Chorus | 提供游戏状态观测与测试断言 |

例如，一个完整的游戏断线重连测试可以这样进行：

**Wingman** 创建 1,000 名模拟玩家

**FaultDeck** 对指定玩家连接注入 5 秒故障

**Chorus** 记录房间状态、重连和恢复结果

**FaultDeck** 汇总故障时间线及业务断言报告

这实际上比单纯模拟网络更有价值。

未来还可以利用同样的机制对 Cockpit 管理的数据库、容器、虚拟机网络进行可靠性测试，但不需要让 Cockpit 承担故障注入逻辑。

## 七、必须重视故障恢复和安全隔离

这是整个工具最不应该省略的部分。

FaultDeck 有能力直接把服务器网络弄断。如果没有完善的隔离和回滚，很容易造成测试机器失联。

我建议强制具备以下设计：

| 安全机制 | 行为 |
|--------|--------|
| Dry Run | 执行前展示具体影响的 IP、端口、网络接口 |
| Target Allowlist | 默认只允许显式授权的测试目标 |
| Max Duration | 每个实验必须设置最长持续时间 |
| Local Watchdog | 本机独立监督，不依赖远程控制器在线 |
| Rollback Journal | 修改前记录状态及恢复操作 |
| Management Exclusion | 排除 SSH、ZeroTier 等管理连接 |
| Experiment Lease | 实验超时自动停止 |
| Conflict Detection | 检测现有 tc、防火墙及其他故障规则冲突 |
| Audit Log | 记录谁、何时、对什么目标实施了故障 |

尤其是你的 Linux 服务器通过 SSH 和 ZeroTier 管理，**不能直接对承载管理流量的网卡施加全局 netem 规则**。

第一版最好采用隔离网络命名空间或 TCP 代理模式。这样即使实验出了问题，也尽量只影响被测试的连接。

本地 Watchdog 也不能只是远程控制器里的一个定时任务。必须在目标机器本地负责到期清理，并提供独立的应急恢复方式。

## 八、我建议的 MVP 开发计划

P0

**最小可用版本：TCP 故障实验**

Rust CLI、Toxiproxy Adapter、实验 YAML、延迟/断线/黑洞/带宽限制、自动恢复和 JSON 报告。

目标：能真正自动复现游戏连接故障，不依赖 GUI。

---

P1

**Linux 原生网络故障**

增加 tc/netem Adapter、TCP/UDP、流量选择、突发丢包、乱序、隔离网络命名空间。

目标：支持真正的 UDP/KCP 网络故障测试。

---

P2

**游戏场景与自动化集成**

提供游戏弱网、战斗断线、跨服隔离模板，集成 Wingman API、外部断言和测试报告。

目标：从网络工具升级为可重复执行的可靠性测试工具。

---

P3

**跨平台和远程执行**

Windows WinDivert、可选 Web UI、多目标实验、远程 Agent、历史对比、CI 集成。

目标：独立、轻量的跨平台网络故障测试产品。

### 建议的 Rust 项目组织

```text
faultdeck/
├── crates/
│   ├── faultdeck-core/       # 故障类型、目标、状态模型
│   ├── faultdeck-scenario/   # 实验 DSL 和调度
│   ├── faultdeck-backend/    # Backend Trait
│   ├── faultdeck-toxiproxy/  # TCP 代理适配
│   ├── faultdeck-netem/      # Linux tc/netem
│   ├── faultdeck-safety/     # Watchdog、Lease、Rollback
│   ├── faultdeck-report/     # 指标和报告
│   └── faultdeck-cli/        # CLI
├── examples/
│   ├── tcp-reconnect.yaml
│   ├── udp-loss.yaml
│   └── network-partition.yaml
└── tests/
    ├── integration/
    └── recovery/
```

我会把 **能够可靠撤销故障** 作为 MVP 的硬性验收条件，而不是功能完成后再补。

## 九、这个项目值不值得单独做？

我的判断是：

**值得，但应该把它定位成“轻量故障实验编排器”，而不是新的 Chaos Mesh。**

如果只想在 Linux 上模拟 10% 丢包、200ms 延迟，`tc netem` 已经足够。如果只测试 TCP 数据库超时，Toxiproxy 也已经很好用。

FaultDeck 真正的差异化应该是：

**一个场景文件就能跨平台创建故障 → 自动执行 → 自动恢复 → 结合游戏测试断言生成报告。**

我尤其看好它和 Wingman 的配合。Wingman 可以负责“玩家在做什么”，FaultDeck 负责“网络发生了什么”，游戏服务端提供“系统最终是否正确”的状态证据。

而且你不需要为了 FaultDeck 重新开发一整套分布式 Agent 平台。第一版做到独立 CLI、轻量本地执行和可靠回滚，就已经是一个有实用价值、能够开源的项目了。

---

If you want, I can:

- 继续描述 MVP 开发计划
- 举例详细说明断线重连场景测试的执行流程
- 详细介绍 FaultDeck 与 Wingman、Chorus 集成的技术细节

===== 片段 2 (1278 字) =====

我觉得 **FaultDeck 比单纯的 GameBot 更值得独立开发**，尤其适合你现在的游戏后端项目生态。

但我想把最初的想法升级一下：

\u003e **FaultDeck：一个轻量级、跨平台、可编排的网络故障实验工具。**
\u003e
\u003e 不仅模拟延迟、丢包和断线，还能按照预定义场景自动制造故障、验证应用能否恢复，并生成可复现的测试报告。

它应该同时面向游戏开发、分布式系统、微服务和普通网络应用，而不是绑定 KBEngine 或某一个游戏服务器。

## 一、FaultDeck 到底能解决什么问题？

假设你的游戏服务器出现以下情况：

- 玩家进入房间后突然断网，重连时发现角色还停留在旧房间。
- 战斗结算响应丢失，客户端重试后奖励发放两次。
- 两台服务器之间的网络断开，双方都认为自己仍然拥有房间管理权。
- 网络抖动造成玩家操作延迟、位置回滚、状态不同步。
- Redis 或数据库连接突然超时，游戏逻辑被阻塞。
- 主播与游戏服务器短暂失联，恢复后房间里的其他参与者状态异常。

这些问题通过正常的单元测试很难完整复现。

FaultDeck 的目的就是让你能够指定：

**什么时候发生故障 → 影响哪些连接 → 持续多久 → 怎样恢复 → 系统恢复后应该满足什么条件。**

## 二、它可以模拟哪些故障？

| 故障 | 具体效果 | 游戏场景 |
|---|---|---|
| Latency | 固定延迟、随机抖动 | 高延迟玩家操作 |
| Packet Loss | 随机丢包、连续突发丢包 | Wi-Fi/移动网络不稳定 |
| Reorder | 网络包乱序 | UDP/KCP 帧同步 |
| Duplicate | 网络包重复 | UDP 消息去重 |
| Corruption | 修改少量数据位 | 协议异常处理 |
| Bandwidth | 限速、突发带宽下降 | 弱网资源下载 |
| Disconnect | TCP RST、连接关闭 | 强制断线 |
| Blackhole | 静默丢弃流量 | 半开连接、心跳超时 |
| Partition | 指定节点间网络隔离 | 跨服通信、脑裂 |
| DNS Failure | DNS 解析故障 | 依赖服务连接 |
| Flapping | 网络反复断开恢复 | 频繁重连 |
| Asymmetric | 上行正常、下行异常 | 单向网络故障 |

有一个重要的技术区别：

**TCP 丢包不等于游戏业务消息丢失。** TCP 通常会重传并保持字节顺序，所以 TCP 场景中更多表现为延迟、吞吐量下降和超时；而 UDP/KCP 会直接受到丢包、乱序、重复报文影响。

因此 FaultDeck 必须区分网络层故障和应用层故障，不能简单地把两者混在一起。

## 三、现成开源工具有哪些？还值得自己做吗？

我查了几种成熟方案。

\u003cbox gap={3}\u003e
  \u003crow align=\\

===== 片段 3 (312 字) =====

/\u003e
    \u003c/box\u003e
  \u003c/row\u003e
\u003c/box\u003e

**所以，我并不建议 FaultDeck 重新发明一个类似 netem、Toxiproxy 或 clumsy 的底层网络引擎。**

真正值得开发的，是它们上面缺少的一层：

**统一的故障描述、跨平台执行适配、游戏场景模板、自动恢复和结果验证。**

## 四、我最想做的功能：故障场景编排

单纯提供一个延迟滑块，或者输入 `loss 10%`，意义有限。

真正有价值的是能描述一个完整的实验。

### 示例：玩家匹配时突然断网

\u003cbox border radius=\\

===== 片段 4 (1375 字) =====

\u003e验证成功重连、没有重复角色、房间所有权一致、玩家状态未损坏\u003c/text\u003e
      \u003c/box\u003e
    \u003c/row\u003e
  \u003c/box\u003e
\u003c/box\u003e

这个实验要支持自动运行，可以设计一份配置：

```yaml
apiVersion: faultdeck.dev/v1alpha1
kind: Experiment

metadata:
  name: game-reconnect-test

target:
  name: game-gateway
  driver: toxiproxy
  protocol: tcp
  direction: both

safety:
  environment: staging
  maxDuration: 60s
  autoRollback: true

steps:
  - at: 0s
    action: baseline

  - at: 10s
    action: blackhole
    duration: 12s

  - at: 22s
    action: recover

  - at: 30s
    action: verify
    checks:
      - player_reconnected
      - room_state_consistent
      - no_duplicate_session
```

这是我建议的 DSL 草案，真正执行前还需要配置目标代理地址和对应的业务验证接口。

其中 `player_reconnected` 等业务断言不能由纯网络引擎凭空推断，需要 Wingman、测试客户端或游戏服务提供相应的观测信息。

### 更值得预置的游戏故障场景

| 场景模板 | 故障方式 | 重点验证 |
|---|---|---|
| 玩家弱网 | 高延迟 + 抖动 + 丢包 | 移动同步、客户端预测 |
| 登录断线 | 连接建立期间断开 | 登录状态和会话清理 |
| 战斗断线 | 战斗中网络黑洞 | 重连、快照恢复 |
| 帧同步异常 | UDP 乱序 + 丢包 | 帧序号、补帧、回滚 |
| 跨服隔离 | GameServer 与协调节点断联 | 房间所有权、状态一致性 |
| 数据库超时 | DB 连接延迟或无响应 | 降级、超时、数据可靠性 |
| 支付响应丢失 | 回调链路中断 | 重试与发货幂等 |
| 主播网络抖动 | 网络反复中断 | 主播状态恢复、房间清理 |

这里要把网络实验和业务故障实验区分开。例如“HTTP 请求重试后重复发奖”属于业务幂等验证，不能仅凭制造重复 TCP 数据包来模拟。FaultDeck 只负责故障，业务测试由应用侧适配器提供。

---

## 五、我建议的技术架构

你喜欢 Rust，又有 Windows、Linux、多台服务器的环境。我认为 Rust 非常适合实现 FaultDeck 的控制核心，但没必要用 Rust 重写 Linux 内核已经拥有的网络模拟机制。

\u003cbox border radius=\\

===== 片段 5 (607 字) =====

]}/\u003e

一个很重要的实现原则是：**实验 DSL 描述“故障语义”，Backend 决定“怎样实现”。**

例如 `loss: 10%`：

- Linux netem：真正的网络包丢失。
- Toxiproxy：丢失代理处理的数据块，语义不等同于 IP 包丢失。
- Windows WinDivert：网络包级丢弃。

如果某个 Backend 不支持要求的精确语义，就应该返回“不支持”，而不是悄悄做一个近似实现。

## 六、和你现有项目怎样配合？

我不建议把 FaultDeck 直接合并进 Wingman 或 Cockpit。

这三个产品的职责不同。

| 项目 | 应该负责什么 |
|---|---|
| **FaultDeck** | 故障注入、实验执行、恢复、故障报告 |
| [Wingman](https://github.com/cuihairu/wingman) | 自动化玩家行为、启动测试工作流 |
| [Cockpit](https://github.com/cuihairu/cockpit) | 提供基础设施节点信息和监控入口 |
| Kode | 调试 KBEngine 开发环境、分析协议 |
| Chorus | 提供游戏状态观测与测试断言 |

例如，一个完整的游戏断线重连测试可以这样进行：

\u003cbox border radius=\\

===== 片段 6 (908 字) =====

\u003e
      **FaultDeck** 汇总故障时间线及业务断言报告
    \u003c/box\u003e
  \u003c/row\u003e
\u003c/box\u003e

这实际上比单纯模拟网络更有价值。

未来还可以利用同样的机制对 Cockpit 管理的数据库、容器、虚拟机网络进行可靠性测试，但不需要让 Cockpit 承担故障注入逻辑。

## 七、必须重视故障恢复和安全隔离

这是整个工具最不应该省略的部分。

FaultDeck 有能力直接把服务器网络弄断。如果没有完善的隔离和回滚，很容易造成测试机器失联。

我建议强制具备以下设计：

| 安全机制 | 行为 |
|---|---|
| Dry Run | 执行前展示具体影响的 IP、端口、网络接口 |
| Target Allowlist | 默认只允许显式授权的测试目标 |
| Max Duration | 每个实验必须设置最长持续时间 |
| Local Watchdog | 本机独立监督，不依赖远程控制器在线 |
| Rollback Journal | 修改前记录状态及恢复操作 |
| Management Exclusion | 排除 SSH、ZeroTier 等管理连接 |
| Experiment Lease | 实验超时自动停止 |
| Conflict Detection | 检测现有 tc、防火墙及其他故障规则冲突 |
| Audit Log | 记录谁、何时、对什么目标实施了故障 |

尤其是你的 Linux 服务器通过 SSH 和 ZeroTier 管理，**不能直接对承载管理流量的网卡施加全局 netem 规则**。

第一版最好采用隔离网络命名空间或 TCP 代理模式。这样即使实验出了问题，也尽量只影响被测试的连接。

本地 Watchdog 也不能只是远程控制器里的一个定时任务。必须在目标机器本地负责到期清理，并提供独立的应急恢复方式。

## 八、我建议的 MVP 开发计划

\u003cbox gap={3}\u003e
  \u003crow align=\\

===== 片段 7 (1207 字) =====

\u003e目标：独立、轻量的跨平台网络故障测试产品。\u003c/text\u003e
    \u003c/box\u003e
  \u003c/row\u003e
\u003c/box\u003e

### 建议的 Rust 项目组织

```text
faultdeck/
├── crates/
│   ├── faultdeck-core/       # 故障类型、目标、状态模型
│   ├── faultdeck-scenario/   # 实验 DSL 和调度
│   ├── faultdeck-backend/    # Backend Trait
│   ├── faultdeck-toxiproxy/  # TCP 代理适配
│   ├── faultdeck-netem/      # Linux tc/netem
│   ├── faultdeck-safety/     # Watchdog、Lease、Rollback
│   ├── faultdeck-report/     # 指标和报告
│   └── faultdeck-cli/        # CLI
├── examples/
│   ├── tcp-reconnect.yaml
│   ├── udp-loss.yaml
│   └── network-partition.yaml
└── tests/
    ├── integration/
    └── recovery/
```

我会把 **能够可靠撤销故障** 作为 MVP 的硬性验收条件，而不是功能完成后再补。

## 九、这个项目值不值得单独做？

我的判断是：

**值得，但应该把它定位成“轻量故障实验编排器”，而不是新的 Chaos Mesh。**

如果只想在 Linux 上模拟 10% 丢包、200ms 延迟，`tc netem` 已经足够。如果只测试 TCP 数据库超时，Toxiproxy 也已经很好用。

FaultDeck 真正的差异化应该是：

**一个场景文件就能跨平台创建故障 → 自动执行 → 自动恢复 → 结合游戏测试断言生成报告。**

我尤其看好它和 Wingman 的配合。Wingman 可以负责“玩家在做什么”，FaultDeck 负责“网络发生了什么”，游戏服务端提供“系统最终是否正确”的状态证据。

而且你不需要为了 FaultDeck 重新开发一整套分布式 Agent 平台。第一版做到独立 CLI、轻量本地执行和可靠回滚，就已经是一个有实用价值、能够开源的项目了。

---

If you want, I can:

- 继续描述 MVP 开发计划
- 举例详细说明断线重连场景测试的执行流程
- 详细介绍 FaultDeck 与 Wingman、Chorus 集成的技术细节