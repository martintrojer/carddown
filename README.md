# carddown

A CLI flashcard system with spaced repetition that lives in your text files.

![carddown demo](doc/demo.gif)

Study facts embedded in your markdown notes, tracked by content hash so cards survive edits and file moves. No cloud, no sync, no binary formats — just your files and a JSON database.

- **Scan** — extract flashcards from any markdown/text file
- **Revise** — interactive TUI with spaced repetition (SM2, SM5, Simple8)
- **Audit** — review orphaned and leech cards

## Install

Pre-built binaries on the [releases page](https://github.com/martintrojer/carddown/releases), or:

```bash
cargo install carddown
```

## Quick start

```bash
# Mark flashcards in your notes
echo "Who discovered penicillin? : Alexander Fleming 🧠" >> notes.md

# Scan your notes
carddown scan ./notes

# Study due cards
carddown revise

# Review problem cards
carddown audit
```

## Writing flashcards

Single-line — use `🧠` or `#flashcard` as marker:

```markdown
Capital of France? : Paris 🧠
What is HTTP? : HyperText Transfer Protocol #flashcard #web
```

Multi-line — end with `---` or `***`:

```markdown
Explain the Bohr model #flashcard #physics
Electrons orbit in quantized energy levels.
Transitions between levels emit/absorb photons.
---
```

Tags like `#physics` let you focus study sessions: `carddown revise --tag physics`

## Key features

| Feature | |
|---|---|
| **Spaced repetition** | SM2, SM5, and Simple8 algorithms with quality grades 0-5 |
| **Content hashing** | Cards identified by blake3 hash — move files freely |
| **Tags** | Filter study sessions by topic |
| **Incremental scan** | Only re-parses modified files |
| **Leech detection** | Flags cards you repeatedly fail |
| **Reverse cards** | Optionally swap prompt/response with `--reverse-probability` |
| **Cram mode** | Review all cards regardless of schedule |

## Documentation

- **[User Guide](doc/GUIDE.md)** — commands, revise workflow, card format details
- **[Algorithms](doc/ALGORITHMS.md)** — how SM2, SM5, and Simple8 work

## License

MIT

## Acknowledgements

- [NeuraCache Markdown Flashcard Specification](https://github.com/NeuraCache/markdown-flashcards-spaced-repetition)
- [Emacs org-drill mode](https://gitlab.com/phillord/org-drill/)
