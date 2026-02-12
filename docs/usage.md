# codex-spp usage

## Core Commands

- `spp init`: create runtime directories and default state.
- `spp status`: evaluate weekly ratio and print gate status.
- `spp drive start`: start Drive session and begin transcript recording.
- `spp drive stop`: stop active Drive session and finalize transcript.
- `spp drive status`: show Drive mode/session state.
- `spp drive`: shorthand for `spp drive start`.
- `spp pause --hours 24`: pause gate checks temporarily (`--hours` is clamped to `1..24`).
- `spp resume`: clear pause and resume gate checks.
- `spp reset`: reset state (including manual attribution overrides) and clear files in
  `.codex-spp/weekly/`, `.codex-spp/transcripts/`, and `.codex-spp/runtime/`
  (session logs in `.codex-spp/sessions/` are not removed).
- `spp codex`: launch Codex with enforced sandbox/approval flags.
- `spp project init [PROJECT]`: scaffold SPP assets into another project.

## Transcript Logging

- Chat source defaults to `CODEX_HOME/history.jsonl` (or `~/.codex/history.jsonl`).
- Drive transcript files are written to `.codex-spp/transcripts/<session-id>.jsonl`.
- Event types: `session_start`, `chat_user`, `chat_assistant`, `file_diff`, `session_end`.
- Runtime recorder control files are written to `.codex-spp/runtime/`.
- Default poll interval is `2000ms`; increase `[transcript].poll_interval_ms` for larger repositories.

## Attribution

- `spp attrib fix <commit> --actor human`
- `spp attrib fix <commit> --actor ai`

Manual overrides are persisted in `.codex-spp/state.json` and take highest priority.

## Project Bootstrap Example

- `spp project init /path/to/your-project --with-codex-config`
- add `--force` when you want to overwrite existing files.
