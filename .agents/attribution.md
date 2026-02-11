# codex-spp attribution rules

週次 gate の集計に使う帰属ルールを定義する。

## Priority

1. `spp attrib fix` による手動補正
2. commit message trailer
   - 例: `Co-Authored-By: Codex`
3. commit author/email が Codex bot 設定に一致
4. `git notes`（`spp:ai` / `spp:human`）による補助判定

## Manual Correction

- 誤判定がある場合は `spp attrib fix <commit> --actor <human|ai>` を使って補正する。
- 補正情報は `./.codex-spp/state.json` に保存し、再集計時に最優先で適用する。

## Notes Convention

- AI 判定: `spp:ai`
- Human 判定: `spp:human`
