# codex-spp usage

## Core Commands

- `spp init`: create runtime directories and default state.
- `spp status`: evaluate weekly ratio and print gate status.
- `spp drive start`: start Drive session and begin transcript recording.
- `spp drive stop`: stop active Drive session and finalize transcript.
- `spp drive status`: show Drive mode/session state.
- `spp drive`: shorthand for `spp drive start`.
- `spp pause --hours 24`: pause gate checks temporarily (max 24h).
- `spp resume`: clear pause and resume gate checks.
- `spp reset`: reset weekly state and manual attribution overrides.
- `spp codex`: launch Codex with enforced sandbox/approval flags.
- `spp project init [PROJECT]`: scaffold SPP assets into another project.

## Transcript Logging

- Chat source defaults to `CODEX_HOME/history.jsonl` (or `~/.codex/history.jsonl`).
- Drive transcript files are written to `.codex-spp/transcripts/<session-id>.jsonl`.
- Event types: `session_start`, `chat_user`, `chat_assistant`, `file_diff`, `session_end`.
- Runtime recorder control files are written to `.codex-spp/runtime/`.

## Attribution

- `spp attrib fix <commit> --actor human`
- `spp attrib fix <commit> --actor ai`

Manual overrides are persisted in `.codex-spp/state.json` and take highest priority.

## Project Bootstrap Example

- `spp project init /path/to/your-project --with-codex-config`
- add `--force` when you want to overwrite existing files.
