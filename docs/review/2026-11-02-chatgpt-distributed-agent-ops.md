
===== 片段 0 (2723 字) =====
:1}

Papercliphttps://github.com/paperclipai/paperclip 任务治理优先

具备任务分配、原子领取、Agent 角色、预算、审批和审计机制，也支持 Hermes、Codex、Claude Code 适配器。

适合把 Agent 当作长期工作的团队管理，但针对你要求的原生会话随时接管，还需要单独验证执行适配器的能力。citeturn668769search6turn506718search0

这里有一个许可证区别：Multica 虽然公开源码并允许内部自部署，但采用带商业托管与嵌入限制的自定义 Multica License，**不是标准 Apache-2.0 开源许可**。如果你坚持 OSI 意义上的完全开源，应优先考虑 MIT 许可的 Happier、Agent of Empires 或 Paperclip。citeturn837358view1

## 三、按你的需求横向比较

| 关键能力 | Multica | Happier | Agent of Empires | Paperclip |
|--------|--------|--------|--------|--------|
| 多机器统一管理 | ✅ | ✅ | 局部 | 依赖执行适配器 |
| 任务队列、失败重试 | ✅ | 部分 | 需外部调度 | ✅ |
| Claude / Codex | ✅ | ✅ | ✅ | ✅ |
| 实时查看执行过程 | ✅ | ✅ | ✅ | ✅ |
| 人工中途介入 | 支持部分运行时 | ✅ | ✅ | 依赖适配器 |
| Linux 无头 | ✅ | ✅ | ✅ | ✅ |
| Windows Worker | ✅ | ✅ | 不原生支持 | 取决于部署 |
| 手机管理 | Web / iOS | Android / iOS / Web | Web | Web |
| 许可证 | 自定义限制 | MIT | MIT | MIT |

这里的 ✅ 表示项目文档已提供相应功能，不代表已完成你这种 30+ 编码席位规模下的可靠性实测。

### Multica 特别值得注意的地方

它的 Daemon 不是简单地接收一次 HTTP 消息，而是有比较完整的执行生命周期：

**Multica Server**

PostgreSQL · 任务队列 · 状态与审计

Linux

Headless Daemon

---

Codex · Claude

Windows

Desktop Daemon

---

Codex · Claude

macOS

Daemon

---

Codex · Claude

每台机器注册自己的可用 Agent 运行时，领取分配给它的任务

官方文档确认：

- Daemon 每 15 秒发送心跳。
- 默认单 Daemon 最大并发 20 个任务，可配置。
- WebSocket 通知新任务，轮询作为断线补偿。
- 运行时断线有重连宽限期，适用的失败任务可以自动重试。citeturn457795search0

但有个很重要的限制：**任务分配到某台机器后，该机器下线，任务不会自动迁移到另一台机器执行。** 因此，Multica 解决的是多机任务分发和恢复，不是完整的跨节点自动故障转移资源池。

## 四、我更建议你这样选择

### 方案 A：继续使用 Hermes，但替换 Peer 控制通道

Hermes + Happier — 最少改造，优先推荐试用

让 Hermes 继续承担主 Agent 的规划和编排，把 Claude Code/Codex 进程交给 Happier 管理。

这里有一条已经具备技术基础的集成路径：Hermes 支持连接 MCP Server，而 Happier 提供 `happier mcp serve`，可让外部 Agent 创建、查询、中断和发送消息到 Happier 会话。citeturn335686search0turn457795search1

在主机安装并登录 Happier 后，可尝试在 `~/.hermes/config.yaml` 中配置：

```yaml
mcp_servers:
  happier:
    command: happier
    args:
      - mcp
      - serve
```

这样 Hermes 就有机会直接控制由 Happier 管理的编码会话，而不是使用 `peer dm` 传递长任务。

这是根据两个项目已公开接口提出的集成方案，**不是经过端到端实测的官方 Hermes + Happier 集成**。需要验证工具发现、跨机器启动、会话中断及重连行为。

### 方案 B：使用 Multica 作为真正的任务控制中心

如果你的第一优先级是大量任务可靠排队、按执行节点分配、追踪失败和人工审核，Multica 更符合 Worker 的概念。

它的自托管部署可使用 Docker Compose，Linux 无头服务器可以只安装 CLI/Daemon，Windows 也可作为独立执行节点。citeturn921043view0turn811749view0

需要接受的是：它比 Happier 重一些，许可证也不属于标准开源许可。

## 五、我的最终选择

对于你现在的情况，我建议**先测试 Happier，不再继续花大量精力修补 Hermes Peer**。

原因是你最在意的不只是执行任务，还包括长时间保持 Claude/Codex 会话、随时从手机介入、继续交互，以及让 Hermes 精确地控制已有编码席位。Happier 已经提供了其中大量现成能力。

如果后续重点转到从 `todo.md` 批量生成任务、数十个 Worker 自动领取、任务重试和跨节点调度，再认真评估 Multica 的任务系统。

但我不会说它们已经完整解决你的所有问题：**Happier 更像分布式 Agent 会话控制系统；Multica 更像多节点 Agent 任务系统。** 把这两者的能力集于一体，才是你最终想要的架构。

---

If you want, I can:

- 继续讨论Happier与Hermes集成的具体实施步骤
- 探讨Multica应用于多节点任务调度的优势与限制
- 介绍将Hermes与其他开源项目集成的可能策略

===== 片段 1 (1090 字) =====
**有。Hermes 现在已经有比 Peer 更适合 Worker 的机制，但还没有完全解决你需要的分布式编码 Agent 管理问题。**

我查了截至 2026 年 11 月 1 日的 Hermes 官方文档和 GitHub Issues，发现一个重要变化：

**Hermes Kanban 已经支持持久化任务队列、独立 Worker 进程、失败恢复和任务重新领取。** 这比单纯通过 Peer 给另一个 Hermes 发送消息，更接近你想要的 Worker 模式。citeturn647332search12

不过要区分三种机制：

| 机制 | 核心能力 | 适合你的程度 |
|--------|--------|--------|
| Hermes Peer | 跨机器 Agent 消息通信 | 不推荐作为主要任务调度机制 |
| Hermes `delegate_task` | 并行启动子 Agent | 适合短期、同一运行环境内的委派 |
| Hermes Kanban | 持久化任务、独立 Worker、失败重试 | Hermes 内部最值得考虑的方向 |

这里最关键的区别是：**Agent 能互相发送消息，不等于具备可靠的分布式 Worker 调度能力。**

你的目标实际上需要机器注册、能力匹配、任务持久化、执行状态追踪、可靠取消、失败恢复，以及允许人随时介入正在运行的 Claude Code/Codex 会话。

## 一、Hermes Peer 不稳定，有实际的架构原因

我找到了一条与你的现象高度相关的 Issue：

Hermes \#118208：Peer 长任务阻塞其他任务https://github.com/NousResearch/hermes-agent/issues/118208

2026 年 9 月的报告指出，多次 `peer run` 可能进入同一个 Bot Chat 会话，并按照会话锁串行执行。一个长任务就可能让后面的请求等待很久，甚至直到约 30 分钟的等待限制触发。citeturn647332search1

另外，A2A 通信也出现过调用端超时早于远端任务完成的问题，导致调用者误以为任务失败，重试时可能重复启动任务。citeturn647332search4turn647332search14

Hermes 现在提供了更合适的异步调用形式：

```bash
hermes peer run linux-01 \
  --idempotency-key task-001 \
  \

===== 片段 2 (790 字) =====


hermes peer status linux-01 run_abc123

hermes peer stop linux-01 run_abc123
```

其中 `peer run` 立即返回执行 ID，`status` 查询结果，幂等键用于避免同一请求重试时重复创建运行。相比长时间等待 `peer dm`，这是更合理的用法。citeturn647332search3

但它仍然不是完整 Worker 系统。

### Hermes Kanban 还有两个关键限制

官方文档明确写出：

- **Kanban 是单主机设计。** SQLite 看板和 Dispatcher 默认部署在本地，不支持多台机器直接共享一个看板来协调 Worker。
- **Codex CLI、Claude Code CLI 的外部 Worker Lane 尚未形成开箱即用的官方集成。** 虽然提供了可扩展的 `spawn_fn` 接口，但还要自行实现启动、状态回报、生命周期处理。citeturn822911search1turn503913view1

而所谓的 *Multi-Gateway Deployment*，主要解决多个 Hermes Profile Gateway 的 Dispatcher 所有权问题，不代表已经有分布式 Worker 资源池。citeturn503913view0

这意味着，单纯从 Peer 切换到 Kanban，能改善本机任务管理，却无法直接完成你的 Windows + Linux + macOS 多节点统一调度。

## 二、值得重点看的开源项目

进一步检索后，我找到了一个比 Hermes Peer 更接近你需求的项目：**Multica**。另一个非常值得比较的是 **Happier**。

image_group{\

===== 片段 3 (616 字) =====
/>

不过要区分三种机制：

| 机制 | 核心能力 | 适合你的程度 |
|---|---|---|
| Hermes Peer | 跨机器 Agent 消息通信 | 不推荐作为主要任务调度机制 |
| Hermes `delegate_task` | 并行启动子 Agent | 适合短期、同一运行环境内的委派 |
| Hermes Kanban | 持久化任务、独立 Worker、失败重试 | Hermes 内部最值得考虑的方向 |

这里最关键的区别是：**Agent 能互相发送消息，不等于具备可靠的分布式 Worker 调度能力。**

你的目标实际上需要机器注册、能力匹配、任务持久化、执行状态追踪、可靠取消、失败恢复，以及允许人随时介入正在运行的 Claude Code/Codex 会话。

## 一、Hermes Peer 不稳定，有实际的架构原因

我找到了一条与你的现象高度相关的 Issue：

[Hermes #118208：Peer 长任务阻塞其他任务](https://github.com/NousResearch/hermes-agent/issues/118208)

2026 年 9 月的报告指出，多次 `peer run` 可能进入同一个 Bot Chat 会话，并按照会话锁串行执行。一个长任务就可能让后面的请求等待很久，甚至直到约 30 分钟的等待限制触发。<Cite ref=\

===== 片段 4 (573 字) =====
/>

## 三、按你的需求横向比较

| 关键能力 | Multica | Happier | Agent of Empires | Paperclip |
|---|---|---|---|---|
| 多机器统一管理 | ✅ | ✅ | 局部 | 依赖执行适配器 |
| 任务队列、失败重试 | ✅ | 部分 | 需外部调度 | ✅ |
| Claude / Codex | ✅ | ✅ | ✅ | ✅ |
| 实时查看执行过程 | ✅ | ✅ | ✅ | ✅ |
| 人工中途介入 | 支持部分运行时 | ✅ | ✅ | 依赖适配器 |
| Linux 无头 | ✅ | ✅ | ✅ | ✅ |
| Windows Worker | ✅ | ✅ | 不原生支持 | 取决于部署 |
| 手机管理 | Web / iOS | Android / iOS / Web | Web | Web |
| 许可证 | 自定义限制 | MIT | MIT | MIT |

这里的 ✅ 表示项目文档已提供相应功能，不代表已完成你这种 30+ 编码席位规模下的可靠性实测。

### Multica 特别值得注意的地方

它的 Daemon 不是简单地接收一次 HTTP 消息，而是有比较完整的执行生命周期：

<box border radius=\

===== 片段 5 (513 字) =====
]}/>

需要接受的是：它比 Happier 重一些，许可证也不属于标准开源许可。

## 五、我的最终选择

对于你现在的情况，我建议**先测试 Happier，不再继续花大量精力修补 Hermes Peer**。

原因是你最在意的不只是执行任务，还包括长时间保持 Claude/Codex 会话、随时从手机介入、继续交互，以及让 Hermes 精确地控制已有编码席位。Happier 已经提供了其中大量现成能力。

如果后续重点转到从 `todo.md` 批量生成任务、数十个 Worker 自动领取、任务重试和跨节点调度，再认真评估 Multica 的任务系统。

但我不会说它们已经完整解决你的所有问题：**Happier 更像分布式 Agent 会话控制系统；Multica 更像多节点 Agent 任务系统。** 把这两者的能力集于一体，才是你最终想要的架构。<MemoryCite />

---

If you want, I can:

- 继续讨论Happier与Hermes集成的具体实施步骤
- 探讨Multica应用于多节点任务调度的优势与限制
- 介绍将Hermes与其他开源项目集成的可能策略

===== 片段 6 (481 字) =====
]}/>

在主机安装并登录 Happier 后，可尝试在 `~/.hermes/config.yaml` 中配置：

```yaml
mcp_servers:
  happier:
    command: happier
    args:
      - mcp
      - serve
```

这样 Hermes 就有机会直接控制由 Happier 管理的编码会话，而不是使用 `peer dm` 传递长任务。

这是根据两个项目已公开接口提出的集成方案，**不是经过端到端实测的官方 Hermes + Happier 集成**。需要验证工具发现、跨机器启动、会话中断及重连行为。

### 方案 B：使用 Multica 作为真正的任务控制中心

如果你的第一优先级是大量任务可靠排队、按执行节点分配、追踪失败和人工审核，Multica 更符合 Worker 的概念。

它的自托管部署可使用 Docker Compose，Linux 无头服务器可以只安装 CLI/Daemon，Windows 也可作为独立执行节点。<Cite refs={[\
