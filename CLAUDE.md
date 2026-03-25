# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build & Run

```bash
cargo build                          # debug build
cargo test                           # run all tests (121 tests)
cargo run -- scan <path>             # extract flashcards from files/folders
cargo run -- revise                  # start interactive study session
cargo run -- audit                   # review orphaned/leech cards
cargo install --path .               # install locally
```

Debug logging: `RUST_LOG=debug cargo run -- scan <path>`

## Before Committing

Always run these before committing:

```bash
cargo fmt
cargo clippy -- -D warnings
```

CI runs fmt check, clippy with `-D warnings`, and tests on both ubuntu and macos.

## Architecture

Carddown is a CLI flashcard system with spaced repetition that extracts cards from text files.

**Data flow:**

```
Scan: text files → parse_file() → Card structs → update_db() → cards.json
Revise: cards.json → filter_cards() → revise::App TUI → update statistics
Audit: cards.json → filter orphan/leech → audit::App TUI → delete cards
```

**Module responsibilities:**

| Module | Role |
|---|---|
| `card.rs` | Parse flashcards from markdown. Single-line `Q : A 🧠 #tag` and multi-line with `---`/`***` separators. Cards identified by blake3 hash of content (survives file moves). |
| `vault.rs` | Vault root discovery: walks up from cwd/scan path looking for `.carddown/`, `.git/`, `.hg/`, `.jj/`. `VaultPaths` resolves all file paths. |
| `db.rs` | SQLite storage in `.carddown/carddown.db`. Cards, global state, and scan index in one file. JSON import/export for migration. |
| `algorithm/` | SM2, SM5, Simple8 spaced repetition. Quality grades 0-5 where 0-2 are failures. `CardState` tracks ease_factor, interval, repetitions, failed_count. |
| `view/revise.rs` | Ratatui TUI for review sessions. `ReviseConfig` groups session params. Requires reveal before grading. Shows status messages for leech/expiry. |
| `view/audit.rs` | Ratatui TUI for card cleanup. Colored status messages (success/error/info). Blocks leech deletion with feedback. |
| `view/formatting.rs` | Shared formatting helpers. `format_tags()` returns sorted output for determinism. |
| `main.rs` | CLI (clap), scan/revise/audit/import/export commands, `filter_cards()`, incremental scan index, lock mechanism. |

**Key design decisions:**

- **Content hashing for identity**: Cards are identified by blake3 hash of their content, not file path or line number. This means cards survive file moves and renames. Changing the hash input would silently orphan all existing cards — the `test_card_id_stability` test guards against this.
- **Per-vault storage**: Data lives in `.carddown/carddown.db` (SQLite) at the vault root, discovered by walking up from cwd or scan path. Each vault is independent — `--vault` overrides discovery. The database file is safe to version control.
- **SQLite storage**: Cards, global state, and scan index all live in one `carddown.db` file with WAL mode. JSON import/export available via `import` and `export` commands for migration and backup.
- **Lock file prevents concurrent instances**: A lock file in `.carddown/` prevents multiple carddown processes from corrupting the database. `--force` overrides.
- **Incremental scanning**: The `scan_index` table tracks file mtimes. Only modified files are re-parsed unless `--full` is used.
- **Reveal-before-grade invariant**: The revise TUI requires pressing space to reveal the answer before any quality grade (0-5) is accepted. `try_grade()` enforces this.
- **Tag filtering happens before the TUI**: `filter_cards()` in `main.rs` handles tag/orphan/leech/interval filtering. The revise `App` receives pre-filtered cards.
- **Reverse map computed at construction**: `reverse_probability` determines which cards show response-as-prompt. The `reverse_map` is computed once in `App::new()`, not per-interaction.
- **Output convention**: `stdout` is reserved for data output (export). All user-facing status messages (summaries, migration, warnings) go to `stderr`. `log::debug!` for developer tracing (`RUST_LOG=debug`). `log::warn!` only for unexpected internal errors (e.g., scan index write failures).

## CI/CD

- **CI** (`.github/workflows/ci.yml`): Lint (fmt + clippy), test (ubuntu + macos), coverage (codecov)
- **Release** (`.github/workflows/release.yml`): Triggered by `v*` tags. Builds linux-x86_64 and macos-aarch64 binaries, creates GitHub release with tarballs.

To release: bump version in `Cargo.toml`, commit, tag with `git tag -a v{version} -m "v{version}"`, push with `--tags`.
