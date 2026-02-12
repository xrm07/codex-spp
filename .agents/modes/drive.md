# Drive Mode

Drive Mode は「人間が運転し、AI はナビとして伴走する」ための制約モード。

## Expected Behavior

- AI は質問、方針、チェックリスト、検証観点を優先して出す。
- AI の出力は最小限の疑似コードまでに抑え、完成コードの提示を避ける。
- 変更作業は人間主体で行い、AI はレビューと次の一手を案内する。

## Runtime Enforcement

- `spp codex` は Drive Mode 時に `--sandbox read-only --ask-for-approval on-request` で起動する。
- 週次 gate 未達なら自動で Drive Mode に遷移する。
