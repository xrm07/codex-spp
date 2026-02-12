# codex-spp

codex-spp is a Skill/Practice Protocol (SPP) toolkit for Codex CLI.  
It keeps development human-led by combining policy files, Codex skills, and a wrapper CLI that enforces safe execution mode based on weekly learning metrics.

## Goals

- Keep implementation decisions and hands-on coding human-led.
- Use AI as coach/navigation support when practice quality drops.
- Make learning progress observable with structured logs and weekly reports.

## Core Features

- `Normal` / `Drive` mode operations aligned with SPP policy.
- Weekly gate evaluation with `human:ai ratio` metrics.
- Automatic shift to Drive mode when weekly ratio falls below target.
- Drive session transcripts with boundary events (`session_start` / `session_end`).
- Chat ingestion from Codex history (`history.jsonl`) plus saved-file diff capture.
- Safe Codex launch wrapper (`spp codex`) with enforced sandbox and approval flags.
- Attribution system for commits (manual override, trailer, author/email, git notes).
- Persistent logs under `./.codex-spp/` with JSON schema definitions.

## Architecture Overview

- `AGENTS.md`
  SPP hard constraints that Codex should always follow.
- `.agents/`
  Source-of-truth policy, mode rules, attribution rules, schemas, and primary skills.
- `skills/`
  Compatibility mirror for Codex skill discovery.
- `crates/spp/`
  Rust implementation of the `spp` wrapper CLI.
- `.codex-spp/` (runtime, git-ignored)
  State file, session logs, and weekly reports generated during operation.

## Prerequisites

- Rust toolchain
- Git
- Codex CLI (`@openai/codex`)

## Quick Start

```bash
# 1) Build wrapper CLI
cargo build -p spp

# 2) Prepare runtime config
cp template_spp.config.toml .codex-spp/config.toml

# 3) Optional: project codex config template
cp template_spp.codex.config.toml .codex/config.toml

# 4) Initialize runtime directories/state
cargo run -p spp -- init

# 5) Check current weekly gate status
cargo run -p spp -- status

# 6) See enforced codex command without launching it
cargo run -p spp -- codex --dry-run
```

## Command Reference

```text
spp init
spp status
spp drive              # alias of `spp drive start`
spp drive start
spp drive stop
spp drive status
spp pause --hours <1..24>
spp resume
spp reset
spp codex [--dry-run] [EXTRA...]
spp project init [PROJECT] [--with-codex-config] [--force]
spp attrib fix --actor <human|ai> <commit>
```

### What each command does

- `init`
  Creates runtime directories and default state/config when missing.
- `status`
  Computes weekly metrics, evaluates gate, writes weekly report, updates mode if needed.
- `drive start`
  Starts a Drive session boundary, writes `session_start`, and launches transcript recorder.
- `drive stop`
  Stops the active Drive recorder, writes `session_end`, and closes the session.
- `drive status`
  Shows mode and active Drive session metadata.
- `pause`
  Temporarily bypasses gate enforcement for up to 24 hours.
- `resume`
  Clears active pause and resumes gate checks.
- `reset`
  Resets state and clears weekly report files.
- `codex`
  Applies gate logic, logs session metadata, and launches Codex with enforced flags.
- `project init`
  Scaffolds SPP assets into a target project directory (`AGENTS.md`, `.agents`, `.agents/skills`, `skills`,
  `.codex-spp/config.toml`, and `.gitignore` rule for `/.codex-spp/`).
- `attrib fix`
  Saves manual attribution override for a commit hash.

### Bootstrap another project with one command

```bash
# from any directory
spp project init /path/to/your-project --with-codex-config
```

- Default behavior skips existing files.
- Add `--force` to overwrite existing files.

## Mode and Gate Behavior

- Weekly ratio formula:
  `ratio = human_lines_added / (human_lines_added + ai_lines_added)`
- Weekly scope:
  Current ISO week only (Monday 00:00 to next Monday 00:00, UTC), no merge commits.
- If ratio is below target and no active pause:
  mode is forced to `drive` with reason `gate`.
- If ratio recovers and mode was gate-forced drive:
  mode returns to `normal`.
- If no added lines exist in the week:
  ratio is treated as `1.0`.

## Attribution Priority

Commit ownership is classified in this order:

1. Manual override from `spp attrib fix`
2. Commit message trailer (`Co-Authored-By: Codex`)
3. Commit author email match (`[attribution].codex_author_emails`)
4. `git notes` marker (`spp:ai` / `spp:human`)

If none match, the commit is treated as `human`.

## Safety Rules Enforced by `spp codex`

- `--sandbox` is always controlled by `spp` (cannot be overridden).
- `--ask-for-approval` is always controlled by `spp` (cannot be overridden).
- `--full-auto` is prohibited by policy.
- Default mode profiles:
  - `normal`: `workspace-write` + `on-request`
  - `drive`: `read-only` + `on-request`

## Configuration

Runtime config file: `.codex-spp/config.toml`  
Template: `template_spp.config.toml`

Main settings:

- `log_schema_version`
- `weekly_ratio_target`
- `max_log_bytes`
- `diff_snapshot_enabled`
- `[codex.normal]` / `[codex.drive]`
- `[transcript]` (chat source, history path, capture options, watcher excludes)
- `[attribution].codex_author_emails`

Tip: for large repositories, increase `[transcript].poll_interval_ms` to reduce recorder I/O load.

## Logs and Data Layout

- `.codex-spp/state.json`
  Current mode, pause state, attribution overrides, updated timestamp.
- `.codex-spp/sessions/<year>-W<week>.jsonl`
  Session-level Codex launch logs.
- `.codex-spp/transcripts/<session-id>.jsonl`
  Drive session transcript events (`session_*`, `chat_*`, `file_diff`).
- `.codex-spp/runtime/<session-id>.control|.done`
  Recorder control/summary files for active session lifecycle.
- `.codex-spp/weekly/<year>-W<week>.json`
  Weekly metric report and gate result.

Schemas:

- `.agents/schemas/template_spp.session.schema.json`
- `.agents/schemas/template_spp.transcript_event.schema.json`
- `.agents/schemas/template_spp.weekly_report.schema.json`

Log retention is controlled by `max_log_bytes`; oldest log files are pruned when exceeding the limit.

## Repository Structure

```text
.
├── AGENTS.md
├── .agents/
├── crates/
│   └── spp/
├── docs/
├── skills/
├── template_spp.config.toml
├── template_spp.codex.config.toml
└── requirement.md
```

## Related Docs

- `docs/install.md`
- `docs/usage.md`
- `docs/philosophy.md`
- `requirement.md`

## Development Notes

```bash
# CLI help
cargo run -p spp -- --help

# Test/build check
cargo test -p spp
```
