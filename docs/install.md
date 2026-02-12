# codex-spp install

## Prerequisites

- Rust toolchain
- Git
- Codex CLI (`@openai/codex`)

## Setup

1. Build CLI:
   - `cargo build -p spp`
   - (global install option) `cargo install --path crates/spp`
2. Initialize runtime directories:
   - `cargo run -p spp -- init`
3. Prepare runtime config:
   - `cp template_spp.config.toml .codex-spp/config.toml`
   - keep `[transcript].chat_source = "history_jsonl"` for Drive transcript capture
4. (Optional) Prepare Codex project config template:
   - `mkdir -p .codex`
   - `cp template_spp.codex.config.toml .codex/config.toml`

## Verify

- `cargo run -p spp -- status`
- `cargo run -p spp -- codex --dry-run`
- `cargo run -p spp -- drive status`

## Bootstrap another repository

- `spp project init /path/to/target-project --with-codex-config`
