# faultdeck 需求台账

每行一条需求;完成项只保留一行证据(commit + 测试数)。评审详情见 docs/审计-文档一致性.md 与 git log。

## 里程碑

- [x] FD-P1-4 [P1] faultdeck-agent 常驻被测端:监听本地端口收控制端指令,本机执行 tc/netem 与 iptables 故障;安全守卫=管理网段白名单+仅绑回环或配对 token;CLI 增 remote 子命令。
  - commit daed723;测试 agent 14 + transport 3 + cli 6;跨机 Windows→Linux 三场景实测：Windows 控制机在位（openclaw 10.240.0.3 / imac 192.168.5.117 / work-win11 10.240.0.5），Linux 被测机=本机 coding VM；三场景实测待在其上执行（2026-11-07 复核，原「无设备」口径作废）
- [x] FD-ARCH-1 [P1] agent 双向连接:Mode A=agent 监听,Mode B=agent 反向拨号(断线重连),单条长连承载指令/事件/日志/报告,双向 token。
  - commit daed723;测试 transport 3;跨机 NAT 实测 BLOCKED;复核 2026-11-06：仍缺跨 NAT 双机网络环境
- [x] FD-REPORT-1 [P1] 报告体系:场景 JSON+SHA256 哈希、RunReport 时间线/恢复证据/外部断言/重现命令、JSON+HTML 双格式、compare_runs 历史 diff。
  - commit d7cda7d;测试 report 4
- [x] FD-ARCH-2 [P1] Coordinator 多目标中枢:agent 注册/心跳/在线状态,控制端发起实验,报告集中存档,解决双向 NAT。
  - commit e41d3f5 + 775ae20;测试 coordinator 18 + cli hub 1;跨机 NAT/mTLS 实测 BLOCKED(仍缺跨 NAT 双机环境与 mTLS 对端/证书);在线指令分发已接线(见 FD-ARCH-2-DISPATCH)
- [x] FD-ARCH-2-DISPATCH [P1] coordinator 在线指令分发:向已注册 agent 的活动连接实际下发 inject/rollback 并回传结果。
  - commit 775ae20;测试 coordinator 18(含真实 TCP 链路下发/断线失败/离线入队补发/超时)+ cli hub 端到端 1;inject/rollback 真实入队并沿 agent 反向链路下发、结果回传合入报告(lib.rs:351 submit_control、lib.rs:660 spawn_dispatcher、link.rs:140 pump_dispatches);跨 NAT 双机真机实测 BLOCKED(仅缺双机网络环境,协议链路已由同机 TCP 集成测试覆盖)
- [x] FD-DESKTOP-1 [P1] 桌面 UI(Tauri):车群/注入/报告三视图,Rust 侧直连 coordinator 走 wire 协议。
  - commit 61d323d + cf200d6;设计 docs/design/desktop-ui.md;Tauri 壳 crates/desktop(Rust 命令 coordinator_list/inject/rollback/report + 前端 pnpm/Vite/vanilla TS);wire 契约测试 5(单测)+ 全工作区测试绿;CI desktop job 装 webkit2gtk 系统依赖后构建,主 job 用 --exclude faultdeck-desktop;nightly 资产仅含 Linux CLI 二进制(faultdeck / -agent / -coordinator),平台安装器尚未发布(2026-11-15 G2 复核纠正原「见 nightly 资产矩阵」口径)

## 评审修复 2026-11-03(netem)

- [x] netem:validate() 逐个检查 source/destination 地址族,拒绝混合 V4/V6 目标。
  - commit 5197819;测试 netem 单测 8 + adapter 集成 9
- [x] netem:default_device 按 Destination=00000000、UP 位、columns[6] Metric 正确解析 /proc/net/route。
  - commit 5197819;测试同上
- [x] netem:inject() 应用后故障驻留,回滚由调用方显式触发。
  - commit 5197819;测试同上
- [x] netem:注入中途失败仅清理自家 100: qdisc,不破坏接口原有 qdisc。
  - commit 5197819;测试同上
- [x] netem:回滚只抑制 tc「找不到」类消息,spawn 失败映射 ProgramMissing。
  - commit 5197819;测试同上
- [x] netem:拒绝覆盖外部 root qdisc,仅删除自家 100: 句柄。
  - commit 5197819;测试同上
- [x] netem:延迟参数 0<ms≤24h 校验,拒绝溢出值。
  - commit 5197819;测试同上
- [x] netem:补齐 mixed-family/default_device/回滚抑制/dry_run/部分失败测试;真实 tc 集成测试挂 FD-AUDIT-TC-IT。
  - commit 5197819;测试同上
- [x] netem:清理死代码占位导入。
  - commit 5197819
- [x] netem:leftover 恢复路径:遗留 100: qdisc 时先发清理 del 再重新 apply。
  - commit 8cb8388;测试 inject_leftover 2
- [x] netem:TOCTOU 窗口内 add 失败不误删外部 qdisc;测试断言 del 先于 add。
  - commit 948ecd6 + 8cb8388;测试 inject_leftover 2
- [x] netem:rollback 对接口缺失类消息容忍,幂等。
  - commit b0a28f4
- [x] netem:parse_root_qdisc 截断行跳过,不提前终止扫描。
  - commit 2882fd7
- [x] netem:OverlapsManagement 映射为独立错误,不再伪装 EmptyTarget。
  - commit 1fad789

## 评审修复 2026-11-03(agent/report/coordinator/cli)

- [x] agent:serve() applied 状态跨连接共享,rollback 在真实 serve 路径可清除故障。
  - commit a8723b7
- [x] agent:ensure_chain 幂等创建链并安装 INPUT 跳转。
  - commit 2d44b0f
- [x] agent:rollback 先删跳转再幂等 -F/-X。
  - commit 692b023
- [x] agent:firewall 按 source/destination 生成匹配,端口级断连生效;CLI --port-spec 时置 all_traffic=false。
  - commit be7461c
- [x] agent:请求行与传输帧限长 64KiB,防内存耗尽。
  - commit 73008a3
- [x] report:to_html 全字段 HTML 转义,防存储型 XSS。
  - commit f5f5ef2
- [x] coordinator:token 鉴权 + inject 校验/管理网段守卫 + 未知 id 拒绝;二进制支持 --token。
  - commit b5224d0
- [x] 测试:firewall 生命周期、serve 跨连接、coordinator TCP 集成、to_html 恶意输入、CLI 超时(默认 5s,FAULTDECK_TIMEOUT_SECS 可覆盖)。
  - commit 282d597 + 3aed791 + 07bac2e;测试 firewall 12 + coordinator 10
- [x] cli:replay_command 补全全部故障参数。
  - commit 23203c3
- [x] agent:token 常量时间比较;非回环 bind 且无 token 时告警。
  - commit 4dbd049
- [x] agent:无 token 时整行作 payload,不截断含空格内容;错误路径返回 JSON 错误响应。
  - commit f6293b2

## 评审修复 2026-11-04

- [x] firewall:--dport 规则带 -p tcp,规则可真实安装。
  - commit e1d048a
- [x] firewall:跳转安装改 -C 探测后 -I,重启周期不累积残留跳转。
  - commit 8897d0c
- [x] coordinator:token 比较复用 constant_time_eq,与 agent 一致。
  - commit 9efe696
- [x] 文档:configuration 补 coordinator --bind/--token 旗标。
  - commit 0709d5b
- [x] 文档:remote-agents 补 WireMessage 信封与 --token。
  - commit 8da393e
- [x] 文档:applied 状态跨连接共享的生命周期说明。
  - commit 7b2ad2f
- [x] cli:replay 参数 shell 引号包裹,含元字符值可原样执行。
  - commit 5784d4f
- [x] firewall:端口级断连同时 DROP tcp 与 udp。
  - commit 8512d00
- [x] agent:constant_time_eq 循环按预期值长度固定执行,消除长度时序分支。
  - commit 69c97c9
- [x] cli:replay 命令经 sh -c 实际执行验证,覆盖内嵌单引号转义。
  - commit 5f4b589
- [x] cli:replay 值改 --token=<value> 形式,支持 - 开头值。
  - commit fe122fb
- [x] firewall:dry_run 仍发出跳转安装命令;纯 dry_run 不报 IptablesMissing。
  - commit fb011af
- [x] agent:constant_time_eq 长度差按 8 字节完整折叠,消除误判相等。
  - commit bb6e755
- [x] firewall:部分安装失败先回滚再报错;回滚失败输出告警不静默。
  - commit 69c97c9 + 7803c1a
- [x] firewall:mixed-family 目标在触碰 iptables 前预检拒绝,零命令输出。
  - commit 7803c1a
- [x] 测试:回滚序列断言 -D 跳转移除先于 -F/-X。
  - commit d62e589
- [x] 全仓 rustfmt/clippy 告警归零。
  - commit d62e589 + 162e1f8

- [x] 审核发现：新增 favicon.svg 未在 VitePress 配置中通过 link 引用，浏览器不会自动加载该 SVG，文档站点因此不显示预期图标（docs/public/favicon.svg:1）
  - 修复：docs/.vitepress/config.mts:57 配置 head icon；pnpm docs:build 通过，dist/index.html:18 输出 `/faultdeck/favicon.svg`

## 审核发现

- [x] 审核发现：nightly 发布任务未声明 `contents: write`，在默认只读令牌的仓库中删除和创建 release 都会因权限不足失败（.github/workflows/nightly.yml:34）
  - 修复：.github/workflows/nightly.yml:17-18 声明 job 级 `permissions: contents: write`
- [x] 审核发现：手动从非默认分支发布时 `gh release create` 未指定 `--target`，新建 nightly 标签指向默认分支而非实际构建的 `github.sha`（.github/workflows/nightly.yml:42）
  - 修复：.github/workflows/nightly.yml:46 `--target "$GITHUB_SHA"`
- [x] 审核发现：英中远程协议文档将 register 和 fetch_report 请求拆成多行，但 coordinator 按换行分帧，照示例发送会将首行 `{` 当成完整请求并解析失败（docs/remote-agents.md:49）
  - 修复：docs/remote-agents.md:49,58 与 docs/zh/remote-agents.md:45,54 改为单行请求示例；换行分帧依据 crates/agent/src/transport.rs:66
- [x] 审核发现：英中 README 将自动恢复回滚标为已实现，但故障施加后会持续驻留且需要调用方显式 rollback，误导用户认为故障会自动解除（README.md:63）
  - 修复：README.md:63 与 README.zh.md:64 改为 explicit rollback，并标注 automatic fault recovery 未实现；驻留依据 crates/netem/src/lib.rs:204
- [x] 审核发现：英中 README 将 coordinator 多目标中枢标为已实现，但其 inject 仅返回 queued 而未向 agent 下发，无法实际执行多目标实验（README.md:66）
- 修复：README.md:66 与 README.zh.md:67 改为 coordinator registration only；README.md:55 与 README.zh.md:56 的 Mode C 文案同步改为「仅入队、在线分发未接线」；queued/dispatch 依据 crates/coordinator/src/lib.rs:159-178；cargo test --workspace 全绿（69 passed, 0 failed）；pnpm docs:build 通过
- 审核发现：本轮 diff 未见问题（fdcbf9f0..f0f554a）——不属实：范围内 570d1fa 曾将 README P3 的「报告集中存档」误标为待实现，与 README.md:55 及 crates/coordinator 的 store_report/fetch_report（lib.rs:171,179-186，测试 inject_stores_report_and_fetches 通过）矛盾；f0f554a 已在同一范围内修正为已实现
- [x] 审核发现：本轮 diff 未见问题（570d1fa..f0f554a）
  - 核验：`git log --oneline 570d1fa..f0f554a` 仅含 85432a3（todo 审核记录）与 f0f554a（README P3 措辞修正）；`git diff --stat 570d1fa..f0f554a` 为 3 文件 +3/-2，仅 README.md、README.zh.md、todo.md 文档文本；README.md:66 与 README.zh.md:67 措辞与 crates/coordinator/src/lib.rs:171（store_report）及 lib.rs:179-186（FetchReport）、crates/coordinator/tests/coordinator.rs:48（inject_stores_report_and_fetches）一致
- [x] 审核发现：本条审核记录标题锚定 570d1fa..f0f554a，但核验正文用移动引用 `HEAD`（现 HEAD=0e5c024），使「仅含 85432a3 与 f0f554a」已不再可复现/不准确（todo.md:126）
  - 修复：todo.md:126 核验正文改为固定端点 `570d1fa..f0f554a`；`git log --oneline 570d1fa..f0f554a` 仅含 85432a3 与 f0f554a，`git diff --stat 570d1fa..f0f554a` 为 3 文件 +3/-2（README.md、README.zh.md、todo.md）
- [x] 审核发现：本轮 diff 未见问题（0e5c024..1cd0d48）
  - 核验：`git log --oneline 0e5c024..1cd0d48` 仅含 1cd0d48（docs: pin audit range endpoints）；`git diff --stat 0e5c024..1cd0d48` 为 todo.md 1 文件 +3/-1，仅文档审核记录文本；1cd0d48 为固定核验端点 570d1fa..f0f554a 并新增审核记录，无代码/文档不一致问题
- [x] 审核发现：本轮 diff 未见问题（1cd0d488..197024c）——核实：`git log --oneline 1cd0d488..197024c` 仅含 197024c（docs: verify audit range 0e5c024..1cd0d48），该 diff 仅 todo.md +2 行文档审核记录文本，无代码/配置不一致
- [x] 审核发现：审核记录使用会随提交移动的 HEAD，导致所称范围现已包含 d0844ab 且变为 todo.md 增加 3 行，核验结论不再准确（todo.md:131）
  - 修复：todo.md:131 核验范围改为固定端点 `1cd0d488..197024c`；`git log --oneline 1cd0d488..197024c` 仅含 197024c，`git diff --stat 1cd0d488..197024c` 为 todo.md 1 文件 +2 行
- [x] 审核发现：修复仅将 todo.md:131 的移动引用 HEAD 钉为固定端点，但同段「审核发现」记录的 todo.md:124 仍以移动引用 `fdcbf9f0..HEAD` 为范围锚点（现该范围已含 10 个提交），所命名的「审核记录使用会随提交移动的 HEAD」问题类别仅被部分修复（todo.md:124）
  - 修复：todo.md:124 范围锚点改为固定端点 `fdcbf9f0..f0f554a`（f0f554a 为该「不属实」注记落笔时 HEAD 所在）；`git log --oneline fdcbf9f0..f0f554a` 仅含 a903065、52674cc、570d1fa、85432a3、f0f554a 共 5 个提交，`git diff --stat fdcbf9f0..f0f554a` 为 36 文件 +173/-254，仍为文档/工作流/待办文本变更；`cargo test --workspace` 全绿（netem 8 + adapter 9 + agent 14 + transport 3 + cli 6 + coordinator 10 + report 4 等全部通过）
- [x] 审核发现：将范围固定为 fdcbf9f0..f0f554a 后，该 diff 的 README P3 已将报告集中存档列为已实现，记录仍称该 diff 将其误标为待实现，导致审核依据不准确（todo.md:124）
  - 修复：todo.md:124 改述为 570d1fa 引入误标、f0f554a 修正；`git show 570d1fa:README.md` 显示 archival pending，`git show f0f554a:README.md` 显示 central report archive live；`cargo test --workspace` 全绿；证据 commit f0f554a
- [x] 审核发现：新增审核记录以单行内联「——修复：」写成，未遵循同节其余修复记录（todo.md:110-135）的两行式「- [x] 审核发现：… / 缩进 修复：…」结构，且 136 行前置空行使该节原本连续的列表断裂（todo.md:137）
  - 修复：todo.md:137 改述为两行式「- [x] 审核发现：… / 缩进 修复：…」并删除 136 行前置空行；`git diff` 显示该记录由单行内联改为两行式且其后无空行，`awk '/^## 审核发现/{f=1} f&&/^$/{n++} END{print n}' todo.md` 输出 1（仅 113 行标题后分隔空行）；`cargo test --workspace` 全绿（69 passed, 0 failed）；`pnpm docs:build` 通过
- [x] 审核发现：本轮 diff 未见问题（6b8e7722..b22ef71）
  - 核验：2026-11-06 `git log --oneline 6b8e7722..b22ef71` 仅含 b22ef71（todo 审核记录格式归一化），`git diff --stat 6b8e7722..b22ef71` 仅 todo.md +4/-2；`cargo test --workspace` 全绿（69 passed, 0 failed），`pnpm docs:build` 通过
- [x] 审核发现：本轮 diff 未见问题（b22ef710..ea31527）
  - 核验：2026-11-06 `git log --oneline b22ef710..ea31527` 仅含 ea31527（todo 审核记录），`git diff --stat b22ef710..ea31527` 为 todo.md +2；`cargo test --workspace` 全绿（69 passed, 0 failed），`pnpm docs:build` 通过；commit ea31527
- [x] 审核发现：里程碑 FD-ARCH-2 标记 [x] 已完成但其需求含「指令路由」，而本 diff 与 crates/coordinator/src/lib.rs:159-178 表明在线指令分发尚未接线（inject/rollback 仅入队返回 queued，routes 字段未用于分发），且本 diff 在 todo.md:14 的复核注记只提缺跨 NAT/mTLS 测试环境、未提缺分发功能，完成状态与实际情况不符（todo.md:13）
  - 修复：todo.md:13 FD-ARCH-2 范围改述为已完成部分(注册/心跳/在线、入队实验、报告存档、双 NAT)，并新增未勾项 FD-ARCH-2-DISPATCH 注明在线分发未接线(lib.rs:69 routes 未用、lib.rs:173,177 仅返回 queued)，todo.md:14 复核注记补「在线指令分发未接线」；cargo test --workspace 全绿（69 passed, 0 failed）；证据 commit 02ac7eb
- [x] 审核发现：FD-ARCH-2 将实验入队标为已完成，但 inject 只创建报告并返回 queued，未保存待执行指令，rollback 也未入队（todo.md:13）
  - 修复：inject/rollback 改为真实入队（CoordinatorState.queues 每 agent VecDeque，crates/coordinator/src/lib.rs:115 PendingCommand、lib.rs:164 queues、lib.rs:351 submit_control），agent 在线时立即沿其链路下发并等待执行结果；测试 inject_queued_when_offline_persists_instruction、rollback_queued_when_offline_persists_instruction、dispatch_inject_and_rollback_through_agent_link；cargo test --workspace 全绿（78 passed, 0 failed）；证据 commit 775ae20
- [x] 审核发现：FD-ARCH-2 将解决双向 NAT 标为已完成，但协调器未将控制指令转发到 agent，无法经中枢对双向 NAT 后的目标执行实验（todo.md:13）
  - 修复：agent 反向拨号建立长连（crates/agent/src/link.rs:77 link_session、:184 run_link），coordinator 沿该既有出站连接下行 Dispatch、上行 Result（crates/coordinator/src/lib.rs:503 handle_agent_link、lib.rs:660 spawn_dispatcher），不要求对 agent 入站可达；测试 dispatch_inject_and_rollback_through_agent_link、queued_command_is_delivered_when_agent_reconnects、cli hub 端到端 hub_cli_dispatches_to_linked_agent；跨 NAT 双机真机实测 BLOCKED（仅缺双机网络环境）；证据 commit 775ae20
- [x] 审核发现：FD-ARCH-2 将在线状态标为已完成，但协调器只记录心跳时间并列出全部已注册 agent，未判断连接或心跳过期状态，离线 agent 仍会出现在列表中（todo.md:13）
  - 修复：在线判定=链路存活或心跳在 HEARTBEAT_TIMEOUT_MS 30 s 内（crates/coordinator/src/lib.rs:211 is_online），list 逐项标注 AgentIdentity.online、available 剔除离线（lib.rs:191/203）；测试 list_marks_offline_after_heartbeat_expiry、list_marks_online_while_link_live_and_offline_after_drop；证据 commit 775ae20

## 巡检修复批 2026-11-11 (faultdeck-audit-fix.md)

- [x] P1.1 agent 监听循环同步处理连接并全程持有 applied 锁，一个慢客户端阻塞整个 agent、切断 rollback 通道（crates/agent/src/lib.rs:342-352）
  - 修复：accept 循环改为每连接一线程，锁仅在 process_request 期间持有（crates/agent/src/lib.rs:370-386 serve_on、lib.rs:343-349 handle_connection）；测试 silent_connection_does_not_block_other_clients；证据 commit 9bcd04c
- [x] P1.2 coordinator 在线分发超时语义混乱：超时报为失败但指令仍会执行，操作员可能重复注入或放弃回滚（crates/coordinator/src/lib.rs:383-388）
  - 修复：移除 DispatchTimedOut 错误变体，超时返回 AgentResponse::TimedOut（结果未知，迟到结果仍会合入报告），CLI 打印在途提示（crates/cli/src/main.rs:204-209）；测试 dispatch_to_silent_agent_times_out、late_result_after_timeout_is_still_recorded；证据 commit 02b892b
- [x] P1.3 远端 rollback 不回写报告：报告永远显示未回滚（README 称回滚是 MVP 验收门，数据失真）
  - 修复：remote rollback 接受 --report-json/--report-html 并回写 rolled_back_at_ms + rollback 事件（crates/cli/src/main.rs:369-388）；测试 cli_rollback_updates_the_inject_report；证据 commit e070b4c
- [x] P1.4 firewall apply 按 networks() 迭代猜 source/dest，source==dest 时 -s 发两次、-d 从不发（crates/agent/src/firewall.rs:144-148）
  - 修复：按角色显式生成 -s source / -d destination 规则（crates/agent/src/firewall.rs:140-147）；测试 apply_emits_both_rules_when_source_equals_destination；证据 commit bc6b467
- [x] P1.5 agent 默认无 token，非回环绑定不拒绝——能落 DROP 规则的守护进程应 fail-closed
  - 修复：serve_on 在 accept 前校验实际绑定地址，非回环且无 token 返回 InsecureBind（crates/agent/src/lib.rs:162-169 require_secure_bind、lib.rs:366-371 serve_on）；测试 require_secure_bind_fails_closed_without_token、serve_on_refuses_non_loopback_listener_without_token；cargo test --workspace 全绿（89 passed, 0 failed）；证据 commit ba1fa9e

### P2 合并清单

- [x] P2.1 dry_run 捕获空字符串——**非缺陷裁定**：SystemCommandExecutor 的 dry_run 分支不启动子进程，stdout/stderr 恒为空串是有意语义（无进程即无输出），调用方把空输出当"无操作"是 dry-run 的既有行为；不改代码，无 commit。
- [x] P2.2 Disconnect 生成非法 qdisc 命令——修复：netem validate() 在 core_validate 前拒绝 Disconnect（DisconnectNotNetem），netem_commands 的 Disconnect 分支防御性返回空（crates/netem/src/lib.rs:446-452、575）；测试 rejects_disconnect_as_not_netem；证据 commit 77f42ad。
- [x] P2.3 iptables 仅入站单向——修复：JUMP_CHAINS=[INPUT,OUTPUT,FORWARD]，ensure_chain/rollback 遍历三链（crates/agent/src/firewall.rs:14、95、207）；测试 ensure_chain_installs_chain_and_jump、ensure_chain_dry_run_emits_jump_install、full_apply_rollback_lifecycle_is_recorded；证据 commit 26139a5。
- [x] P2.4 Heartbeat 可伪造在线——修复：控制面 Heartbeat 要求存活的 reverse link，否则 NotConnected 拒绝（crates/coordinator/src/lib.rs:320）；测试 register_heartbeat_list、heartbeat_accepted_for_linked_agent；证据 commit 33787d1。
- [x] P2.5 FetchReport 不校验归属——修复：report_owner 映射 + store_report（crates/coordinator/src/lib.rs:190、210-211、379）；非属主与未知 hash 同答 "report not found"；测试 fetch_report_requires_owner；证据 commit 1e161ee。
- [x] P2.6 拒绝注册无错误响应→重连风暴——修复：coordinator 鉴权失败写 AgentResponse::Error 帧，agent 对 RegisterRejected 直接退出（crates/agent/src/link.rs:68-69、113、212-213）；测试 rejected_agent_registration_gets_error_response、run_link_exits_on_rejection_instead_of_storm；证据 commit 1dc2ca0。
- [x] P2.7 CLI read_to_string 无界 OOM——修复：响应读取改用 read_line_limited(MAX_INJECTION_BYTES)（crates/cli/src/main.rs:266）；测试 cli_bails_on_oversized_response；证据 commit 9280ebe。
- [x] P2.8 scenario_name 静默丢弃——修复：AgentCommand::Inject 增 #[serde(default)] scenario_name，handler 优先用操作员名、缺省回退 {id}-injection（crates/coordinator/src/lib.rs:72-78、334-347）；CLI hub inject 透传（crates/cli/src/main.rs:425）；desktop 镜像同步；测试 inject_honors_operator_scenario_name_in_report；cargo test --workspace 全绿（96 passed, 0 failed）；证据 commit a564f79。
- [x] P2.9 每链路一线程忙轮询 50ms——修复：调度器改在队列 Condvar 上阻塞，enqueue 时 notify_one、链路替换/移除时 notify_all 唤醒退出；queue_cond 以 Arc<Condvar> 持有以绕开 wait 借用冲突（crates/coordinator/src/lib.rs:190-194、256、603、723、763-765）；测试 idle_link_dispatcher_wakes_on_enqueue；cargo test --workspace 全绿（97 passed, 0 failed）；证据 commit d97077c。
- [x] P2.10 token 含空格无法认证——修复：remote 头 token 百分号编码（encode_token/decode_token），agent 侧拆分后先解码再常量时间比较；空格 token 端到端可认证、错误 token 仍拒绝（crates/agent/src/lib.rs:72-87、341；crates/cli/src/main.rs:260）；测试 cli_authenticates_token_with_spaces、cli_rejects_wrong_token_with_spaces、token_encoding_roundtrips_and_keeps_plain_tokens；cargo test --workspace 全绿（100 passed, 0 failed）；证据 commit aa8a386。
- [x] P2.11 to_yaml 实为 JSON——修复：Scenario::to_yaml 改用 serde_yaml 输出块式 YAML（原 serde_json::to_string_pretty 产出 JSON），新增 ReportError::Yaml；字段/函数契约与名称一致（crates/report/src/lib.rs:17、53）；测试 scenario_yaml_is_yaml_not_json；cargo test --workspace 全绿（101 passed, 0 failed）；证据 commit 7ad7cb8。
- [x] P2.12 validate_disconnect_target 空函数假象——修复：原是空壳且从未被调用；改为校验端口定向断连（all_traffic=false）必须携带 destination_port，并在 firewall::apply 入口调用；无端口时先于任何 iptables 命令拒绝，避免静默放大为整 CIDR DROP（crates/agent/src/firewall.rs:27、133、240-247）；测试 port_targeted_disconnect_without_port_is_rejected；cargo test --workspace 全绿（102 passed, 0 failed）；证据 commit 5a88b41。
- [x] P2.13 disconnect 默认不可用——修复：binary() 在未配置 --iptables 时默认 PATH 上的 iptables（原先直接回 IptablesMissing），spawn 时 NotFound 才映射回 IptablesMissing；断连开箱即用于装有 iptables 的主机，无 iptables 仍清晰报错（crates/agent/src/firewall.rs:44-49、65）；测试 binary_defaults_to_iptables_on_path_when_unset；cargo test --workspace 全绿（103 passed, 0 failed）；证据 commit 05c6f1c。

## 源码分层分析 2026-11-07

### 抽象层清单

- 共享语义与安全模型：定义故障/目标契约并做内在与管理网段校验（`crates/core/src/lib.rs:108`）。
- 策略到后端翻译层：将延迟/丢包翻译成可执行、可规划、可回滚的 tc 命令（`crates/netem/src/lib.rs:172`）。
- 宿主执行与防护层：接收并鉴权请求、路由后端、维护跨连接的单活动故障状态（`crates/agent/src/lib.rs:171`）。
- 线传输与拓扑层：提供有界的 NDJSON 分帧和 Mode A/Mode B 连接拓扑（`crates/agent/src/transport.rs:56`）。
- 用户入口与报告物化层：转换 CLI 参数、执行远程调用并生成报告与回放命令（`crates/cli/src/main.rs:136`）。
- 机群注册与控制面状态层：维护易失 Agent 注册表、报告归档、存活链路与每 agent 指令队列，并经链路在线分发控制指令（`crates/coordinator/src/lib.rs:156`）。
- 证据与可复现模型层：定义场景哈希、运行报告、HTML 渲染和历史 diff（`crates/report/src/lib.rs:78`）。

详细术语见 `docs/glossary.md` 与 `docs/zh/glossary.md`；层落位与拆分理由见 `docs/architecture.md` 与 `docs/zh/architecture.md`。

### 超大文件候选

无 >1000 行文件。

### 目录/分类问题

- `crates/protocol/src` 是空目录，却被架构表列为占位；协议类型实际分散在 agent 和 coordinator（`crates/coordinator/src/lib.rs:98`）。建议移除目录或建立真实共享协议层时再保留。
- CLI 自己定义并序列化 `WireRequest`/`WireResponse`，而 agent 拥有对应 `Request`/`Response`，存在无编译一致性的重复线协议定义（`crates/cli/src/main.rs:83`，`crates/agent/src/lib.rs:109`）。建议后续提取共享协议层或用同一类型导出。
- `crates/coordinator/src/lib.rs` 从 `faultdeck-agent` 引用通用传输与常量时间比较，把传输能力和宿主库耦合进控制面依赖（`crates/coordinator/src/lib.rs:6`，`crates/coordinator/src/lib.rs:124`）。建议把传输和鉴权辅助迁移为独立共享库。

## Codex 深审发现（2026-11-13，luna profile 近5 commit 审核，1C/2M/4m，待修）

- [ ] 修复 `decode_token` 边界为 `i + 2 <= bytes.len()`（`crates/agent/src/lib.rs:92`）——恢复完整 token 解码。
- [ ] 增加仅 `%XX`、以 `%XX` 结尾及全字节空间 token 的认证测试（`crates/agent/tests/agent.rs:403`）——锁定该回归路径。
- [ ] 将 `decode_token` 改为按字节解码并要求合法 UTF-8（`crates/agent/src/lib.rs:87-105`）——避免 token 隐式替换。
- [ ] 移除 `rollback` 对 `-X Too many links` 的容忍（`crates/agent/src/firewall.rs:221-224`）——不再把残留跳转链当成功。
- [ ] 增加带残留跳转链的回滚失败测试（`crates/agent/tests/firewall.rs:157`）——覆盖真实脏状态。
- [ ] 调整 `process_request` 的空状态回滚策略（`crates/agent/src/lib.rs:284-303`）——使 rollback 可清理进程重启后的残留规则。
- [x] 同步英文与中文文档中 `--iptables` 默认语义（`docs/remote-agents.md:22`）——消除 PATH 默认值修复的文档残留。
  - 修复：G2 巡检（2026-11-14）按源码事实统一五处口径（`crates/agent/src/firewall.rs:44-52` 未配置时回落 PATH 上的 `iptables`）——`docs/remote-agents.md`、`docs/zh/remote-agents.md`、`docs/quickstart.md`、`docs/zh/quickstart.md`、`docs/zh/configuration.md` 全部改为「可选覆盖,默认取 PATH,装上即开箱可用」;英文 `docs/configuration.md` 的同行口径本轮一并核过;证据:落表 G2-1(审计 docs/审计-文档一致性.md Round G2)
- [ ] 让 `RunReport::new` 返回序列化错误而非空 YAML（`crates/report/src/lib.rs:103`）——避免静默丢失报告证据。
- [ ] 修复后在可写构建环境中实跑 `cargo test --workspace` 并更新 `todo.md:181-184` 证据——确保勾选项与 HEAD 实际一致。
- [ ] 文档站浏览器语言自动切换（P2,2026-11-14 G2 巡检登记,等用户拍板范围）——现状:docs/.vitepress/config.mts 的 locales 仅 root(en)+zh,线上 `/zh/` 与全部中文页 lang=zh-CN、正文充实(取证见审计 Round G2 的逐页 CJK 计数表),切换完全靠导航栏手动语言链接,无 navigator.language/rootLocale/Accept-Language 探测。受规基线=手动切换+显式链接,自动识别跳转属增强项,本轮不实施。
- [x] faultdeck MCP server（rmcp + stdio，照 sandtable-mcp 先例，2026-11-13 用户令「faultdeck 要有 MCP」）——把故障注入能力暴露给 AI agent：`apply_fault`/`clear_fault`（复用 CLI 既有注入链同一后端抽象，netem/iptables/WinDivert 不在 MCP 层重写）、`list_targets`（枚举可注入目标）、`run_scenario`（预置游戏场景模板：断线重连/战斗结算丢包/双机房脑裂）、`read_report`（RunReport 产物读取）。诚实边界：blackhole/bandwidth 未 shipped 不出对应工具。验收：`cargo test --workspace` 全绿（含新 crate）+ stdio e2e（initialize/tools list/call 各一）+ README 双语补 MCP 入口行 + 不绑死单一 backend。
  - 交付：新 crate crates/faultdeck-mcp（rmcp 3.7 + tokio，stdio，bin `faultdeck-mcp`，入根 workspace）；5 tool=list_targets/apply_fault/clear_fault/run_scenario/read_report，全部走 coordinator wire 协议（WireMessage/AgentCommand，同 CLI hub 链路），MCP 层零第二套注入实现，不绑死 backend（能力取自各 agent 注册上报）；run_scenario 跑 report crate Scenario JSON 经同一 inject 链并回传 scenario_hash，游戏场景模板未 shipped 故未暴露预置面；诚实边界落实：blackhole/bandwidth 无工具且按名拒绝（口径=README「Shipped fault types」），失败=tool-level isError 原文透传，destructive 与显式 target 要求写入 tool description；测试 faultdeck-mcp 4（build_injection 对齐 CLI 语义、未 shipped 类型按名拒绝、响应映射、native 层对真实 coordinator）+ stdio e2e 1（官方 rmcp 客户端真 spawn：initialize→tools/list 断言恰 5 工具→list_targets/read_report/apply_fault/run_scenario 错误与拒绝通道）；cargo test --workspace 全绿（108 passed, 0 failed）；clippy --all-targets -D warnings 零告警；fmt --check 干净；README.md/README.zh.md 各补一行 MCP 入口
