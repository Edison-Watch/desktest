---
description: "When adding or changing CLI behavior, update --help text and the skill documentation. Apply when modifying CLI commands, flags, subcommands, or user-facing output."
globs:
  - 'src/cli.rs'
  - 'src/main.rs'
alwaysApply: false
---

# Update CLI Help When Behavior Changes

When a new feature fundamentally changes CLI behavior (new subcommand, new flag, changed output format, changed default), you MUST also update:

1. **`src/cli.rs`** — clap `#[arg]` / `#[command]` help text and `after_help` examples
2. **`skills/desktest-skill.md`** — the skill file that documents CLI usage for agents
