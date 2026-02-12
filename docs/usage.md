# codex-spp usage

## Core Commands

- `spp init`: create runtime directories and default state.
- `spp status`: evaluate weekly ratio and print gate status.
- `spp drive`: force mode to drive.
- `spp pause --hours 24`: pause gate checks temporarily (max 24h).
- `spp resume`: clear pause and resume gate checks.
- `spp reset`: reset weekly state and manual attribution overrides.
- `spp codex`: launch Codex with enforced sandbox/approval flags.
- `spp project init [PROJECT]`: scaffold SPP assets into another project.

## Attribution

- `spp attrib fix <commit> --actor human`
- `spp attrib fix <commit> --actor ai`

Manual overrides are persisted in `.codex-spp/state.json` and take highest priority.

## Project Bootstrap Example

- `spp project init /path/to/your-project --with-codex-config`
- add `--force` when you want to overwrite existing files.
