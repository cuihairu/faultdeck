# 场景报告与历史 Diff

场景报告由 `faultdeck-report` 生成,注入成功后由 CLI 落盘。格式:JSON(权威)与自包含的 HTML 渲染。

## 生成报告

```bash
faultdeck remote inject … \
  --scenario-name combat-loss \
  --report-json run1.json \
  --report-html run1.html
```

- `--scenario-name` 标记场景名(默认 `"scenario"`)。
- 报告在 agent 确认 `fault applied` 后**由 CLI 在本地写入**。
- `--report-html` 可选;JSON 先写。

## RunReport 结构

```json
{
  "scenario_name": "combat-loss",
  "scenario_yaml": "name: combat-loss\n…",   // Scenario 的 YAML 序列化(字段名沿用历史命名)
  "scenario_hash": "<sha256 hex>",
  "injected_at_ms": 1730000000000,
  "rolled_back_at_ms": null,
  "events": [],                            // FaultEvent { timestamp_ms, kind, message }
  "restore_evidence": [],                  // RestoreEvidence { timestamp_ms, check, passed, detail }
  "external_assertions": [],               // ExternalAssertion { name, passed, detail }
  "replay_command": "faultdeck remote inject --fault loss --destination … --host … --port …"
}
```

要点(直接来自代码):

- **`scenario_hash`** 是 `Scenario { name, injection }` 序列化后的 SHA-256 —— 同一场景永远得到同一哈希,作为归档与 diff 的身份标识。
- **`replay_command`** 是一条可直接复制执行、复现本次注入的 CLI 命令。
- `events`、`restore_evidence`、`external_assertions` 已进入 schema;当前 CLI 路径写出的这三项为空(coordinator 仅在 inject 入队时归档一份报告桩)。

## HTML 渲染

`RunReport::to_html()` 生成独立页面,含四部分:头部(名称、哈希、注入时间)、**Timeline**、**Restore Evidence**、**External Assertions** 三张表,以及 **Replay** 块(回放命令)。无外部资源、无 JS。

## 历史 Diff

`compare_runs(baseline, comparison)` 返回 `HistoricalDiff`:

```json
{
  "baseline_hash": "…",
  "comparison_hash": "…",
  "changed_fields": { "events": "2 vs 0" }
}
```

对三个列表字段(`events`、`restore_evidence`、`external_assertions`)做结构相等比较,每个有差异的字段记录 `"<基线条数> vs <对比条数>"`。目前用法为**库级**(`faultdeck_report::compare_runs`)—— 尚无 diff 的 CLI 子命令。

## Coordinator 归档

coordinator 收到 `inject` 命令时计算场景哈希,并把新建的 `RunReport` 存入内存归档(`CoordinatorState.reports`,以哈希为键的 `BTreeMap`)。客户端用 `fetch_report` 按哈希取回。归档**仅存内存** —— coordinator 重启即失,也尚无文件系统归档。
