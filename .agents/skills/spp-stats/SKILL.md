---
name: spp-stats
description: Compute and explain codex-spp weekly gate metrics. Use when evaluating human:ai ratio, attribution outcomes, and corrective actions.
---

# spp-stats

週次ゲート指標を一貫した定義で評価するスキル。

## Metric Definitions

- `human_lines_added`
- `ai_lines_added`
- `human_commit_count`
- `ai_commit_count`
- `ratio = human / (human + ai)`

## Attribution Priority

1. manual fix (`spp attrib fix`)
2. commit trailer (`Co-Authored-By: Codex`)
3. codex bot author/email match
4. git notes (`spp:ai`, `spp:human`)

## Guidance

- ratio 未達なら Drive Mode への遷移理由を説明する。
- 次週に ratio を改善する具体策を 2〜3 個提示する。
