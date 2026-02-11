
---

## Codex-spp 要件定義書 v1.0（as-of: 2026-02-12 JST）

### 0. 背景・目的

#### 0.1 背景（移植元の観測）

移植元 `claude-spp` は、Claude Code 向けの **SPP（Skill/Practice Protocol）プラグイン**として、以下を提供します。

* **weekly goal / human:ai ratio** を追跡し、比率が目標未達なら **“code output をブロック”**（Drive Mode へ）([GitHub][1])
* **Drive Mode**：人間が運転（実装）し、AI はナビ（質問・次の一手）に徹する。**コードを書かない**([GitHub][1])
* **コーチング**：会話ログや学習ログを `./.claude-spp` に保存し、後で振り返れる ([GitHub][1])
* **制御コマンド**：`spp drive` / `spp pause|resume|reset`、pause には **24h の期限**がある ([GitHub][1])

#### 0.2 移植版の目的

Codex 環境で同等の成果（＝“人間主体で上達するための強制力＋可観測性”）を得る。

* **人間主体の実装ドライブ**を維持しつつ、AI を **コーチ/ナビ**として利用可能にする
* **週次ゲート（人間比率）**を Codex 上で再現（未達なら「自動的に安全モード＋ノーコード」へ）
* ログを構造化し、**再現可能な学習サイクル**にする

---

## 1. ターゲット環境と準拠

### 1.1 対象プロダクト

* Codex CLI（Linux/macOS/Windows想定、主にローカル運用）

  * Codex CLI はターミナル上で動き、対象ディレクトリ内の読取・編集・コマンド実行が可能([OpenAI Developers][2])
* Codex IDE extension（VS Code/Cursor 等）は v1.0 では “互換対象” とする（必須ではない）

### 1.2 Codex 側の準拠ポイント（設計制約）

* **プロジェクト指示**：`AGENTS.md` / `AGENTS.override.md` による階層指示。検索順・結合順が規定される([OpenAI Developers][3])
* **Skills**：`skills/<skill>/SKILL.md`（YAML front matter の `name`/`description` 必須）で定義。説明文で暗黙起動され得る（progressive disclosure）([OpenAI Developers][4])
* **設定**：`~/.codex/config.toml` と `./.codex/config.toml`（リポジトリ単位）で層別設定([OpenAI Developers][5])
* **安全性**：`--sandbox read-only|workspace-write` と `--ask-for-approval ...` の組合せで実行権限を制御できる（`--full-auto` 等）([OpenAI Developers][6])
* **Rules**：`./codex/rules/*.rules` で sandbox 外実行のコマンド制御が可能だが **experimental**([OpenAI Developers][7])

---

## 2. Codex-spp の全体像（アーキテクチャ）

### 2.1 提供物（deliverables）

Codex-spp は「**リポジトリ同梱の規約＋skills＋任意のラッパCLI**」で構成する。

1. **リポジトリ同梱ドキュメント**

* `AGENTS.md`（Codex が確実に読む、規範の“真”）
* `.agents/`（オープン規格寄りの“正本”領域、後述）
* `docs/`（インストール、運用、理念、FAQ）

2. **Codex Skills**

* `skills/spp-drive/SKILL.md`（Drive Mode 運用）
* `skills/spp-coach/SKILL.md`（振り返り支援・問いの設計）
* `skills/spp-stats/SKILL.md`（週次集計・ゲート判定）

3. **任意のラッパ CLI（推奨）**

* `spp`（Rust 推奨 / Node でも可）
  役割：モード切替、週次集計、ログ保存、Codex 起動時の安全フラグ選択（read-only など）

> なぜラッパが必要か：Codex の AGENTS.md/Skills だけでは「**出力の絶対禁止**」「**週次ゲートの機械的強制**」を完全には担保しにくい。安全フラグ切替（read-only）を“外側”から固定するのが最も堅い。([OpenAI Developers][6])

---

## 3. `.agents/` を意識した“オープン規格寄り”設計

### 3.1 目的

* Claude / Codex / 他エージェントで **同一の規範資産を再利用**できるようにする
* Codex 固有ファイル（`AGENTS.md`, `skills/*/SKILL.md`）は **生成物（ビルド成果）**として扱える構造にする

### 3.2 ディレクトリ規約（提案）

* `.agents/`：エージェント規範の正本

  * `.agents/policy.md`：SPP の原則、禁止事項、ログ方針
  * `.agents/modes/drive.md`：Drive Mode の仕様（コード禁止など）
  * `.agents/modes/normal.md`：通常モード仕様
  * `.agents/schemas/*.json`：ログ/週次レポートの JSON schema（後述）
  * `.agents/attribution.md`：人間/AI のコミット帰属ルール

* `AGENTS.md`（Codex 向けの薄いブリッジ）

  * 内容は極力短くし、`.agents/policy.md` を参照する（ただし Codex は参照先を自動読込しないので、**重要部分は AGENTS.md にも重複記載**する）
  * あるいは `project_doc_fallback_filenames` を使い `.agents.md` を instructions として読ませる運用を「オプション」として提示([OpenAI Developers][3])

> 注：Codex の fallback filename 機構で `.agents.md` を instructions として扱える（設定が必要）。([OpenAI Developers][3])

---

## 4. 機能要件（Functional Requirements）

### FR-1: モード管理

* **Normal Mode**：通常のコーディング支援（ただし “人間主体” を維持）
* **Drive Mode**：AI はコードを書かず、質問・方針・チェックリスト・小さな擬似コードまでに限定
* **Pause/Resume/Reset**

  * Pause は **最大 24h**（移植元に準拠）([GitHub][1])
  * Pause 中はゲート判定を無効化できる（例：締切直前の例外）
  * Reset は週次状態（ログ/メタデータ）の再初期化

### FR-2: 週次ゲート（human:ai ratio）

* 週単位（ISO week 推奨）で以下を算出：

  * `human_lines_added`, `ai_lines_added`（または commit 単位でも可）
  * `human_commit_count`, `ai_commit_count`
  * `ratio = human / (human + ai)` を算出し目標と比較
* 目標（例：0.70）を下回った場合：

  1. Codex-spp が **Drive Mode に強制遷移**
  2. 以降の Codex 起動は **read-only** をデフォルトにする（後述）([OpenAI Developers][6])

#### 帰属（attribution）ルール（v1.0 の現実解）

* **優先順位**

  1. コミットメッセージの trailer（例：`Co-Authored-By: Codex`）
  2. commit author/email が codex 用 bot 設定に一致
  3. `spp` が付与する `git notes`（任意）
* “完全自動の真偽判定” は困難なので、**誤判定時の手動修正**（`spp attrib fix ...`）を要件化する

### FR-3: Codex 実行権限の強制（安全制御）

* Drive Mode / ゲート未達時の Codex 起動は、原則として次の組合せ：

  * `--sandbox read-only --ask-for-approval on-request`（対話しつつ変更を防ぐ）([OpenAI Developers][6])
* 通常時：

  * `--sandbox workspace-write --ask-for-approval on-request`（Codex の推奨プリセット相当）([OpenAI Developers][6])

> これにより「AI が勝手に編集してしまう」事故を OS サンドボックスで抑止できる。([OpenAI Developers][6])

### FR-4: コーチングログ（学習の可観測性）

* 保存先：`./.codex-spp/`（移植元が `./.claude-spp` であることに倣う）([GitHub][1])
* 保存対象

  * セッションログ（prompt/response の要約＋モード＋安全設定＋git 状態）
  * 週次サマリ（統計＋達成/未達＋次週の処方）
  * 任意：ファイル差分のスナップショット（容量制限必須）
* データ形式：JSON Lines（`*.jsonl`）＋週次 `report.json`（schema は `.agents/schemas` に置く）

### FR-5: Skill 提供（Codex Skills）

Codex の skills 仕様に準拠し、各スキルは以下を満たす：

* `skills/<name>/SKILL.md` に **YAML front matter（name/description）** を持つ([OpenAI Developers][4])
* 期待されるスキル

  * `spp-drive`：Drive Mode の厳格運用（コード禁止、質問テンプレ、タスク分解）
  * `spp-coach`：振り返り（どこで迷ったか、次に何を練習すべきか）
  * `spp-stats`：週次ゲート計測と改善策提示（指標の定義を固定）

### FR-6: Rules（任意・v1.0 ではオプション）

* `./codex/rules/*.rules` で「sandbox 外で実行してよいコマンド」を絞る（例：`git status` は許可、`rm -rf` 系は prompt）
* ただし Rules は experimental のため、**要件として必須化しない**([OpenAI Developers][7])

---

## 5. 非機能要件（Non-Functional Requirements）

### NFR-1: 安全性（Safety by default）

* デフォルトは **network off** / **workspace 限定** / **on-request approval** を維持([OpenAI Developers][6])
* `--full-auto` の利用は “明示 opt-in” とし、ゲート未達時は禁止（`--full-auto` は便利だが無承認で書けるため）([OpenAI Developers][8])

### NFR-2: 再現性（Reproducibility）

* 設定は `./.codex-spp/config.toml` と `./.codex/config.toml`（Codex 標準）に分離([OpenAI Developers][5])
* ログは schema を固定し、バージョニングする（`log_schema_version`）

### NFR-3: 可搬性（Portability）

* Linux/macOS は必須。Windows は “動作することが望ましい” の扱い。
* ファイル監視は OS ごとの差異が大きいので、監視は v1.0 で “任意機能” に落とす（必須は git ベース集計）

### NFR-4: 性能・容量

* ログ容量の上限（例：500MB）とローテーション
* diff スナップショットは既定 OFF（ON の場合も上限必須）

---

## 6. 最終リポジトリ構造（Codex-spp 移植版）

### 6.1 期待構造（案）

```
/ (repo root)
  AGENTS.md
  .codex/
    config.toml
  .agents/                    # “正本”（オープン規格寄り）
    policy.md
    attribution.md
    modes/
      drive.md
      normal.md
    schemas/
      session.schema.json
      weekly_report.schema.json
  skills/                     # Codex skills（準拠）
    spp-drive/
      SKILL.md
    spp-coach/
      SKILL.md
    spp-stats/
      SKILL.md
  codex/                       # Codex rules（任意）
    rules/
      default.rules
  .codex-spp/                  # 実行時生成（git ignore）
    sessions/*.jsonl
    weekly/*.json
    state.json
  crates/spp/ or packages/spp/ # spp ラッパCLI（任意だが推奨）
  docs/
    install.md
    usage.md
    philosophy.md
```

### 6.2 `AGENTS.md` の設計方針（重要）

* 短く、**“絶対に守る制約” を最上段に**置く（Drive Mode の禁止事項、帰属、ログ方針）
* 詳細は `.agents/` へ参照させるが、**重要ルールは重複**（Codex が参照先を自動で読むとは限らないため）([OpenAI Developers][3])

---

## 7. インストール手順（期待）

### 7.1 前提

* Node.js（Codex CLI を npm で入れる場合）
* git

### 7.2 Codex CLI の導入

Codex CLI は npm で導入できる：([OpenAI Developers][9])

```bash
npm i -g @openai/codex
codex
```

初回起動でサインイン（ChatGPT アカウント or API key）を行う。([OpenAI Developers][9])

### 7.3 リポジトリへの Codex-spp 導入（2パターン）

**A) “ラッパCLIなし（最低限）”**

1. 上記の構造で `AGENTS.md` と `skills/` を追加
2. ゲート未達時は手動で `codex --sandbox read-only ...` を使う（運用）

**B) “ラッパCLIあり（推奨）”**

1. `spp init`：`AGENTS.md` / skills / `.codex/config.toml` / `.agents/` 雛形生成
2. `spp status`：週次比率とモード表示
3. `spp codex`：状態に応じて `codex` を適切な sandbox/approval で起動（強制）([OpenAI Developers][6])

---

## 8. 期待する挙動（シナリオ）

### シナリオ S1：通常運用

1. `spp codex` 起動（workspace-write + on-request）
2. ユーザがタスクを依頼
3. Codex は skills を必要時に呼ぶ（暗黙/明示）([OpenAI Developers][4])
4. ユーザが実装 → コミット
5. `spp status` で週次比率更新

### シナリオ S2：ゲート未達 → 強制 Drive

1. 週次比率が目標未達
2. `spp` が mode を Drive に変更
3. `spp codex` は **read-only** で起動（編集不能）([OpenAI Developers][6])
4. Codex は質問・方針・検証項目のみ（コードブロック禁止）

### シナリオ S3：Pause

1. 例外対応が必要 → `spp pause`
2. 24h のあいだゲート無効([GitHub][1])
3. 期限切れで自動復帰、または `spp resume`

---

## 9. 依存関係（精査）と採用方針

### 9.1 必須依存

| 依存               | 理由       | リスク     | 緩和                                                             |
| ---------------- | -------- | ------- | -------------------------------------------------------------- |
| Codex CLI        | 実行基盤     | CLI仕様変更 | `config.toml`/flags を抽象化して wrapper に隠蔽([OpenAI Developers][5]) |
| git              | 週次集計     | 帰属の曖昧さ  | trailer/author/notes の多段判定＋手動補正                                |
| `AGENTS.md`      | 規範注入     | 長文化で上限  | 指示を分割（override活用）、必要最小限化([OpenAI Developers][3])               |
| Skills（SKILL.md） | 再利用可能な手順 | 発火しない   | `description` のスコープ明確化、明示呼び出し導線([OpenAI Developers][4])        |

### 9.2 任意依存（v1.0 はオプション）

| 依存               | 価値               | 採用条件                                          |
| ---------------- | ---------------- | --------------------------------------------- |
| Rules            | sandbox 外コマンドの制御 | experimental のため既定OFF([OpenAI Developers][7]) |
| ファイル監視（notify 等） | 実装中の可観測性         | OS差分が許容できる場合のみ                                |

---

## 10. 受入条件（Acceptance Criteria）

* `skills/*/SKILL.md` が Codex skills 仕様を満たし、`/skills` 等で認識できる([OpenAI Developers][4])
* `AGENTS.md` が期待通りロードされる（Codex が “読み込んだ指示” を列挙できる）([OpenAI Developers][3])
* ゲート未達時、`spp codex` が **必ず read-only** を選択し、ファイル編集が発生しない([OpenAI Developers][6])
* 週次レポートが schema に準拠して出力される（CI で検証可能）
* Pause が 24h で失効する([GitHub][1])

---

## 11. スコープ外（v1.0）

* “AI が生成したコード断片” を AST レベルで完全同定して帰属する（高コスト＆誤判定が本質的に残る）
* IDE 上のステータスライン完全再現（Claude Code の plugin UI 相当）([GitHub][1])
* Rules を前提とした強制（experimental のため）([OpenAI Developers][7])

---

## 実務上の提案（v1.0 で最も堅い設計）

* **強制力の核**を「プロンプト規約」ではなく「**Codex の sandbox/approval を外側で切る**」に置く（read-only を強制）([OpenAI Developers][6])
* その上で、Drive の “会話品質” は Skills（SKILL.md）で作り込む([OpenAI Developers][4])
* `.agents/` は正本として整備し、Codex 向けには `AGENTS.md`/`skills/` を同期生成する（オープン規格志向）

---

[1]: https://github.com/mlolson/claude-spp "GitHub - mlolson/claude-spp: Pair programming with Claude code. Turns Claude into a pair-programmer/coach who partners with you and helps you learn programming and software engineering."
[2]: https://developers.openai.com/codex/cli/?utm_source=chatgpt.com "Codex CLI"
[3]: https://developers.openai.com/codex/guides/agents-md/ "Custom instructions with AGENTS.md"
[4]: https://developers.openai.com/codex/skills/ "Agent Skills"
[5]: https://developers.openai.com/codex/config-basic/ "Config basics"
[6]: https://developers.openai.com/codex/security/ "Security"
[7]: https://developers.openai.com/codex/rules/ "Rules"
[8]: https://developers.openai.com/codex/cli/reference/ "Command line options"
[9]: https://developers.openai.com/codex/cli?ref=traycer.ai&utm_source=chatgpt.com "Codex CLI"
