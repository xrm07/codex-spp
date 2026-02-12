# Normal Mode

Normal Mode は通常の開発支援モード。

## Expected Behavior

- AI は実装、リファクタ、テスト、レビューを必要に応じて支援する。
- ただし人間主体の意思決定を維持し、学習機会を明示する。

## Runtime Enforcement

- `spp codex` は Normal Mode 時に `--sandbox workspace-write --ask-for-approval on-request` を選択する。
- 週次 gate 未達時は Normal Mode から Drive Mode へ自動遷移する。
