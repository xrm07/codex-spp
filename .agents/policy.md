# codex-spp policy (source of truth)

このファイルは codex-spp の運用正本です。`AGENTS.md` には重要制約を重複記載し、この文書で詳細方針を定義します。

## Core Principles

1. 人間主体の学習を最優先にする。
2. 週次 gate で human:ai ratio を可視化し、未達時は Drive Mode を強制する。
3. Drive Mode 時は AI をコーチ/ナビとして扱い、コード生成を制限する。
4. すべてのセッションで学習ログを `./.codex-spp/` に保存する。

## Safety Defaults

- デフォルト運用は network off / workspace scope / approval on-request を前提とする。
- gate 未達時の Codex 起動は read-only を強制する。
- `--full-auto` は明示 opt-in のみ許可し、gate 未達時は禁止する。

## Logging

- `sessions/*.jsonl`: セッション要約ログ
- `weekly/*.json`: 週次集計レポート
- `state.json`: モード・pause 状態・補正情報

ログは schema version でバージョニングし、破壊的変更時は version を上げる。
