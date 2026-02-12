# codex-spp install

## Prerequisites

- Rust toolchain
- Git
- Codex CLI (`@openai/codex`)

## Setup

1. Build CLI:
   - `cargo build -p spp`
2. Prepare runtime config:
   - `cp template_spp.config.toml .codex-spp/config.toml`
3. (Optional) Prepare Codex project config template:
   - `cp template_spp.codex.config.toml .codex/config.toml`
4. Initialize runtime directories:
   - `cargo run -p spp -- init`

## Verify

- `cargo run -p spp -- status`
- `cargo run -p spp -- codex --dry-run`
