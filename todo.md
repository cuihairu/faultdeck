# faultdeck 需求台账

每行一条需求;完成项只保留一行证据(commit + 测试数)。评审详情见 docs/审计-文档一致性.md 与 git log。

## 里程碑

- [x] FD-P1-4 [P1] faultdeck-agent 常驻被测端:监听本地端口收控制端指令,本机执行 tc/netem 与 iptables 故障;安全守卫=管理网段白名单+仅绑回环或配对 token;CLI 增 remote 子命令。
  - commit daed723;测试 agent 14 + transport 3 + cli 6;跨机 Windows→Linux 三场景实测 BLOCKED(无设备)
- [x] FD-ARCH-1 [P1] agent 双向连接:Mode A=agent 监听,Mode B=agent 反向拨号(断线重连),单条长连承载指令/事件/日志/报告,双向 token。
  - commit daed723;测试 transport 3;跨机 NAT 实测 BLOCKED
- [x] FD-REPORT-1 [P1] 报告体系:场景 JSON+SHA256 哈希、RunReport 时间线/恢复证据/外部断言/重现命令、JSON+HTML 双格式、compare_runs 历史 diff。
  - commit d7cda7d;测试 report 4
- [x] FD-ARCH-2 [P1] Coordinator 多目标中枢:agent 注册/心跳/在线状态,控制端发起实验,指令路由与报告集中存档,解决双向 NAT。
  - commit e41d3f5;测试 coordinator 10;跨机 NAT/mTLS 实测 BLOCKED

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
  - 修复：README.md:66 与 README.zh.md:67 改为 coordinator registration only；queued/dispatch 依据 crates/coordinator/src/lib.rs:159-178
- 审核发现：本轮 diff 未见问题（fdcbf9f0..HEAD）——不属实：该 diff 实含 README P3 将「报告集中存档」误标为待实现，与 README.md:55 及 crates/coordinator 的 store_report/fetch_report（lib.rs:171,179-186，测试 inject_stores_report_and_fetches 通过）矛盾；已修正 P3 措辞
