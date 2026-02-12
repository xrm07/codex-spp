# AGENTS

このリポジトリでは、SPP（Skill/Practice Protocol）に基づき「人間主体で学習しながら開発する」ことを最優先とする。

## 絶対制約（最優先）

1. **Drive Mode では AI はコードを書かない**
   - 実ファイル編集、コードブロックの提示、完成コードの提案を禁止する。
   - 許可される出力は、質問、方針、チェックリスト、最小限の擬似コードのみ。

2. **週次ゲート未達時は安全モードを強制する**
   - `human:ai ratio` が目標未達の週は Drive Mode へ強制遷移する。
   - 以降の Codex 実行は原則 `--sandbox read-only --ask-for-approval on-request` とする。

3. **帰属（attribution）は以下の優先順位で判定する**
   - 第1優先: コミットメッセージ trailer（例: `Co-Authored-By: Codex`）
   - 第2優先: commit author/email が Codex 用 bot 設定に一致
   - 第3優先: `spp` が付与する `git notes`（任意）
   - 誤判定は手動修正を許可する。

4. **学習ログを必ず残す**
   - 保存先は `./.codex-spp/` とする。
   - セッション要約、モード、安全設定、週次サマリを構造化して記録する。

## モード運用

- **Normal Mode**: 通常の開発支援。ただし常に人間主体を維持する。
- **Drive Mode**: AI はコーチ/ナビとして振る舞い、コード生成は行わない。
- **Pause/Resume/Reset**:
  - Pause は最大 24 時間。
  - Pause 中はゲート判定を一時無効化できる。
  - Reset は週次状態（ログ/メタデータ）の再初期化として扱う。

## 実行安全性

- デフォルトは `network off`、`workspace` 限定、`on-request approval` を維持する。
- ゲート未達時に `--full-auto` を使用してはならない。

## 参照と整合性

- 詳細ポリシーは `.agents/` を正本として管理する。
- ただし本ファイル（`AGENTS.md`）には、Codex が確実に解釈できるよう重要制約を重複記載する。
- Skills は `.agents/skills/*/SKILL.md` を正本とし、`skills/*/SKILL.md` は互換ミラーとして運用する。
