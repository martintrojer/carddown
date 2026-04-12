# User Guide

## Commands

Carddown has three commands: **scan**, **revise**, and **audit**.

### Scan

Extract flashcards from files and folders into the carddown database.

```bash
carddown scan ./notes              # scan a folder (incremental)
carddown scan ./notes --full       # full scan (detects orphans)
carddown scan ./notes.md           # scan a single file
```

Options:

| Flag | Default | Description |
|---|---|---|
| `--file-types` | `md txt org` | File extensions to parse |
| `--full` | off | Full scan — also marks deleted cards as orphans |

Incremental scanning (the default) only re-parses files modified since the last scan. Use `--full` periodically to detect cards that were removed from your files.

You can add `@carddown-ignore` anywhere in a file to exclude it from scanning.

Persistent scan defaults can live in `.carddown/config.toml`:

```toml
[scan]
file_types = ["md", "txt", "org"]
```

### Revise

Start an interactive study session with due cards.

```bash
carddown revise                         # review due cards
carddown revise --tag physics           # only physics cards
carddown revise --algorithm sm2         # use SM2 algorithm
carddown revise --cram                  # review all cards
carddown revise --reverse-probability 0.5  # 50% chance to swap Q/A
```

Options:

| Flag | Default | Description |
|---|---|---|
| `--maximum-cards-per-session` | 30 | Max cards per session |
| `--maximum-duration-of-session` | 20 | Session length in minutes |
| `--leech-failure-threshold` | 15 | Failures before marking as leech |
| `--leech-method` | skip | `skip` or `warn` for leech cards |
| `--algorithm` | sm5 | `sm2`, `sm5`, or `simple8` |
| `--tag` | (all) | Filter by tag (repeatable) |
| `--include-orphans` | off | Include orphaned cards |
| `--reverse-probability` | 0.0 | Chance to swap prompt/response |
| `--cram` | off | Ignore intervals, review all cards (doesn't affect stats) |
| `--cram-hours` | 12 | Hours since last review for cram mode |

Persistent revise defaults can live in `.carddown/config.toml`:

```toml
[revise]
maximum_cards_per_session = 30
maximum_duration_of_session = 20
leech_failure_threshold = 15
leech_method = "skip"
algorithm = "sm5"
reverse_probability = 0.0
```

#### Revise workflow

1. A prompt is shown. Try to recall the answer.
2. Press **Space** to reveal the response.
3. Grade your recall from 0 to 5:

| Grade | Meaning | Key |
|---|---|---|
| 5 | Perfect | `5` or `'` |
| 4 | Correct with hesitation | `4` or `l` |
| 3 | Correct with difficulty | `3` or `j` |
| 2 | Incorrect but easy to recall | `2` or `g` |
| 1 | Incorrect but remembered | `1` or `d` |
| 0 | Incorrect and forgotten | `0` or `a` |

Grades 0-2 are failures and reset the card's interval. Press `?` for help, `q` to quit.

### Audit

Review orphaned and leech cards in an interactive TUI.

```bash
carddown audit
```

Navigate with arrow keys (`h`/`k` for left, `l`/`j` for right). Press `d` then `y` to delete orphaned cards. Leech cards cannot be deleted — they should be rewritten in your source files.

### Import

Merge review history from another carddown database into the current vault.

```bash
carddown import ~/.local/state/carddown/cards.json   # import from JSON file
carddown import ../other-vault/.carddown/carddown.db  # import from another SQLite vault
```

Accepts both `.json` (legacy) and `.db` (SQLite) source files. Cards are matched by content hash. Only cards that exist in both databases are updated, and only if the source has more reviews than the target. Safe to run multiple times.

#### Migrating from older versions

**From pre-0.3.0 (global JSON database):** Older versions stored data in `~/.local/state/carddown/`. To migrate:

```bash
cd your-notes/
carddown scan .                                         # creates .carddown/
carddown import ~/.local/state/carddown/cards.json      # import review history
```

**From 0.3.0 (per-vault JSON files):** If your `.carddown/` directory contains `cards.json` instead of `carddown.db`, carddown will auto-migrate on the next run. The old JSON files are kept in place — you can delete them after verifying the migration.

**Merging vaults:** Use `import` to merge review history between vaults. The `export` command can create JSON snapshots for backup or inspection.

### Export

Export the current vault database to JSON files for backup or interoperability.

```bash
carddown export ./backup
```

Writes `cards.json` and `state.json` to the specified directory.

### Global flags

| Flag | Description |
|---|---|
| `--vault <path>` | Override vault root directory |
| `--force` | Override lock file (use if no other instance is running) |

## Card format

### Single-line cards

```markdown
Prompt : Response 🧠
Prompt : Response #flashcard
Prompt : Response 🧠 #tag1 #tag2
```

The `🧠` emoji or `#flashcard` keyword marks a line as a flashcard. Everything before the colon is the prompt, everything after (minus tags) is the response.

### Multi-line cards

```markdown
Prompt text #flashcard #tag1 #tag2
Response line 1
Response line 2
---
```

The prompt is the first line (minus tags). All subsequent lines until the separator are the response. Valid separators:

```
---
- - -
***
* * *
```

### Tags

Tags start with `#` and can contain letters, numbers, hyphens, and underscores. `#flashcard` is reserved as a marker and not stored as a tag.

```markdown
What is DNA? : Deoxyribonucleic acid 🧠 #biology #genetics
```

### Ignoring files

Add `@carddown-ignore` anywhere in a file to skip it during scanning.

## Vaults

Carddown stores config per-vault in `.carddown/` at project root. Vault root is discovered by walking up from current directory (or scan path) looking for `.carddown/`, `.git/`, `.hg/`, or `.jj/`.

```
my-notes/
├── .git/
├── .carddown/          ← created automatically
│   ├── config.toml     ← optional per-vault defaults
│   ├── carddown.db     ← default SQLite database location
│   └── lock            ← default lock file location
├── physics/
│   └── notes.md
└── biology/
    └── notes.md
```

Each vault is independent — scanning and reviewing in one vault never affects another. Use `--vault <path>` to override auto-discovery.

By default all state lives in single `carddown.db` SQLite file under `.carddown/`. Cards are identified by blake3 content hash, so they survive file moves and edits to surrounding text.

If you want SQLite files outside vault, set external state directory:

```toml
[storage]
state_dir = "../carddown-state"
```

Relative paths resolve from vault root. `carddown.db`, `carddown.db-wal`, `carddown.db-shm`, and lock file move there together.

## Terminology

- **Leech** — A card you've failed many times (configurable threshold). Leeches slow your progress and should be rewritten, split into simpler cards, or removed.
- **Orphan** — A card whose content no longer exists in your source files (detected during `--full` scan). Can be deleted via the audit command.
