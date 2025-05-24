# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Development Commands

- `cargo build` - Build the project
- `cargo test` - Run all tests
- `cargo run -- scan <path>` - Extract flashcards from files/folders
- `cargo run -- revise` - Start interactive study session
- `cargo run -- audit` - Review orphaned/leech cards
- `cargo install --path .` - Install locally

## Architecture Overview

Carddown is a CLI flashcard system with spaced repetition that extracts cards from text files.

**Core Data Flow:**
1. Scan text files → Parse flashcards → Store in JSON database
2. Revise session → Filter due cards → Present in TUI → Update statistics
3. Audit mode → Show problematic cards → Allow cleanup

**Key Components:**

- **Card parsing** (`card.rs`): Extracts flashcards from markdown using content hashing (blake3) for identification. Supports single-line format `Question : Answer 🧠 #tag` and multi-line with `---`/`***` separators.

- **Database** (`db.rs`): JSON-based storage in `~/.local/state/carddown/` tracking card metadata, review history, and statistics. Handles orphaned cards and leech detection.

- **Spaced repetition** (`algorithm/`): Implements SM2, SM5, and Simple8 algorithms with quality grades 0-5 where 0-2 are failures that reset progress.

- **Terminal UI** (`view/`): Interactive sessions built with `ratatui` and `crossterm` for reviewing (`revise.rs`) and auditing (`audit.rs`) cards.

**Important Patterns:**
- Cards identified by content hash, survive file moves
- Human-readable JSON storage, no binary formats
- Lock mechanism prevents concurrent instances
- Incremental scanning only checks modified files by default