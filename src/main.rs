mod algorithm;
mod card;
mod db;
mod vault;
mod view;

use crate::algorithm::Algo;
use crate::card::Card;
use crate::db::CardDb;
use crate::db::CardEntry;
use crate::vault::VaultPaths;
use algorithm::new_algorithm;
use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use env_logger::Env;
use rand::prelude::*;
use std::collections::{HashMap, HashSet};
use std::fs::OpenOptions;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;
use walkdir::WalkDir;

/// RAII guard for lock file management
struct LockGuard {
    lock_path: PathBuf,
}

impl LockGuard {
    fn new(lock_path: &Path) -> Result<Self> {
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(lock_path)
        {
            Ok(_) => Ok(LockGuard {
                lock_path: lock_path.to_path_buf(),
            }),
            Err(e) if e.kind() == ErrorKind::AlreadyExists => {
                anyhow::bail!("Another instance of carddown is running, or the previous instance crashed. Use --force to remove the lock file.");
            }
            Err(e) => Err(e.into()),
        }
    }

    fn force_new(lock_path: &Path) -> Result<Self> {
        let _ = std::fs::remove_file(lock_path);
        std::fs::File::create(lock_path)?;
        Ok(LockGuard {
            lock_path: lock_path.to_path_buf(),
        })
    }
}

impl Drop for LockGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.lock_path);
    }
}

type ScanIndex = HashMap<String, u64>; // file path -> mtime seconds

/// Default values for command-line arguments
mod defaults {
    pub const MAX_CARDS_PER_SESSION: usize = 30;
    pub const MAX_DURATION_MINUTES: usize = 20;
    pub const LEECH_FAILURE_THRESHOLD: usize = 15;
    pub const CRAM_HOURS: usize = 12;
    pub const REVERSE_PROBABILITY: f64 = 0.0;
}

fn load_scan_index(path: &Path) -> ScanIndex {
    if let Ok(data) = std::fs::read_to_string(path) {
        serde_json::from_str(&data).unwrap_or_default()
    } else {
        HashMap::new()
    }
}

fn save_scan_index(path: &Path, index: &ScanIndex) {
    match serde_json::to_string(index) {
        Ok(json) => {
            if let Err(e) = std::fs::write(path, json) {
                log::warn!("Failed to write scan index: {e}");
            }
        }
        Err(e) => log::warn!("Failed to serialize scan index: {e}"),
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum LeechMethod {
    Skip,
    Warn,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Scan files for flashcards and add them to the database
    Scan {
        /// File extensions to scan (e.g., md, txt, org)
        #[arg(long, default_values = ["md", "txt", "org"])]
        file_types: Vec<String>,

        /// Perform a complete rescan instead of only checking modified files.
        /// Warning: May generate orphaned cards if files were deleted
        #[arg(long)]
        full: bool,

        /// Path to a file or directory to scan for flashcards
        path: PathBuf,
    },
    /// Review database for problematic cards (orphaned or leech cards).
    /// Orphaned cards: Cards whose source files no longer exist.
    /// Leech cards: Cards that are consistently difficult to remember
    Audit {},
    /// Start a flashcard review session
    Revise {
        /// Limit the number of cards to review in this session
        #[arg(short = 'n', long, default_value_t = defaults::MAX_CARDS_PER_SESSION)]
        maximum_cards_per_session: usize,

        /// Maximum length of review session in minutes
        #[arg(short = 'd', long, default_value_t = defaults::MAX_DURATION_MINUTES)]
        maximum_duration_of_session: usize,

        /// Number of failures before a card is marked as a leech
        #[arg(long, default_value_t = defaults::LEECH_FAILURE_THRESHOLD)]
        leech_failure_threshold: usize,

        /// How to handle leech cards during review:
        /// skip - Skip leech cards entirely.
        /// warn - Show leech cards but display a warning
        #[arg(long, value_enum, default_value_t = LeechMethod::Skip)]
        leech_method: LeechMethod,

        /// Spaced repetition algorithm to determine card intervals
        #[arg(short = 'a', long, value_enum, default_value_t = Algo::SM5)]
        algorithm: Algo,

        /// Only show cards with these tags (shows all cards if no tags specified)
        #[arg(short = 't', long)]
        tag: Vec<String>,

        /// Include cards whose source files no longer exist
        #[arg(long)]
        include_orphans: bool,

        /// Chance to swap question/answer (0.0 = never, 1.0 = always)
        #[arg(short = 'r', long, default_value_t = defaults::REVERSE_PROBABILITY)]
        reverse_probability: f64,

        /// Enable review of all cards not seen in --cram-hours, ignoring intervals
        /// Note: Reviews in cram mode don't affect card statistics
        #[arg(long)]
        cram: bool,

        /// Hours since last review for cards to include in cram mode
        #[arg(long, default_value_t = defaults::CRAM_HOURS)]
        cram_hours: usize,
    },
    /// Import review history from another carddown database.
    ///
    /// Merges card statistics (review count, intervals, leech status) from a
    /// source cards.json into the current vault. Cards are matched by content
    /// hash — only cards that exist in both databases are updated.
    ///
    /// Use this to migrate from an older carddown version (pre-0.2.0) that
    /// stored data globally in ~/.local/state/carddown/, or to merge review
    /// history when reorganising vaults.
    Import {
        /// Path to the source cards.json file to import from
        source: PathBuf,
    },
}

/// CARDDOWN - A command-line flashcard system that manages cards from text files.
/// Cards are extracted from text files and can be reviewed using spaced repetition.
/// The system tracks review history and automatically schedules cards for optimal learning.
///
/// Data is stored in a .carddown/ directory at the vault root (discovered by
/// walking up from the current directory or scan path looking for .carddown/,
/// .git/, .hg/, or .jj/).
#[derive(Parser, Debug)]
#[command(version, about, long_about=None)]
struct Args {
    #[command(subcommand)]
    command: Commands,

    /// Override vault root directory (default: auto-discovered from cwd or scan path)
    #[arg(long)]
    vault: Option<PathBuf>,

    /// Override file locking mechanism.
    /// Warning: Only use if no other carddown instances are running
    #[arg(long)]
    force: bool,
}

fn mtime_secs(path: &std::path::Path) -> Option<u64> {
    std::fs::metadata(path)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
}

fn file_types_to_set(file_types: &[String]) -> HashSet<&str> {
    HashSet::from_iter(file_types.iter().map(|s| s.as_str()))
}

fn collect_files(folder: &Path, file_types: &HashSet<&str>) -> Vec<PathBuf> {
    WalkDir::new(folder)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
        .filter(|e| {
            e.path()
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| file_types.contains(ext))
        })
        .map(|e| e.path().to_path_buf())
        .collect()
}

#[cfg(test)]
fn parse_cards_from_folder(folder: &Path, file_types: &[String]) -> Result<Vec<Card>> {
    let file_types_set = file_types_to_set(file_types);
    collect_files(folder, &file_types_set)
        .into_iter()
        .try_fold(Vec::new(), |mut acc, path| {
            let mut cards = card::parse_file(&path)?;
            acc.append(&mut cards);
            Ok(acc)
        })
}

fn filter_cards(
    db: CardDb,
    tags: HashSet<String>,
    include_orphans: bool,
    leech_method: LeechMethod,
    cram_mode: bool,
    cram_hours: usize,
) -> Vec<CardEntry> {
    let today = chrono::Utc::now();
    db.into_values()
        .filter(|c| is_card_due(c, today, cram_mode, cram_hours))
        .filter(|c| matches_tags(c, &tags))
        .filter(|c| include_orphans || !c.orphan)
        .filter(|c| !should_skip_leech(c, leech_method))
        .collect()
}

fn is_card_due(
    card: &CardEntry,
    today: chrono::DateTime<chrono::Utc>,
    cram_mode: bool,
    cram_hours: usize,
) -> bool {
    match card.last_revised {
        Some(last_revised) => {
            if cram_mode {
                today - last_revised >= chrono::Duration::hours(cram_hours as i64)
            } else {
                let next_date = last_revised + chrono::Duration::days(card.state.interval as i64);
                today >= next_date
            }
        }
        None => true,
    }
}

fn matches_tags(card: &CardEntry, tags: &HashSet<String>) -> bool {
    tags.is_empty() || !card.card.tags.is_disjoint(tags)
}

fn should_skip_leech(card: &CardEntry, leech_method: LeechMethod) -> bool {
    matches!(leech_method, LeechMethod::Skip) && card.leech
}

/// Resolve vault paths from CLI args.
///
/// Priority: `--vault` flag > scan path (for scan command) > cwd.
fn resolve_vault(args: &Args) -> VaultPaths {
    if let Some(vault_path) = &args.vault {
        vault::find_vault_root(vault_path)
    } else if let Commands::Scan { path, .. } = &args.command {
        let start = if path.is_file() {
            path.parent().unwrap_or(path)
        } else {
            path
        };
        vault::find_vault_root(start)
    } else {
        vault::find_vault_root(&std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
    }
}

/// Import review statistics from a source database into the target database.
///
/// Matches cards by content hash. Only updates cards that exist in both databases.
/// Returns the number of cards updated.
fn import_stats(source_path: &Path, target_path: &Path) -> Result<usize> {
    let source_db = db::get_db(source_path)?;
    let mut target_db = if target_path.exists() {
        db::get_db(target_path)?
    } else {
        CardDb::new()
    };

    let mut updated = 0;
    for (id, source_entry) in &source_db {
        if let Some(target_entry) = target_db.get_mut(id) {
            // Only update if source has review history and target doesn't,
            // or source has more reviews
            if source_entry.revise_count > target_entry.revise_count {
                target_entry.state = source_entry.state.clone();
                target_entry.last_revised = source_entry.last_revised;
                target_entry.revise_count = source_entry.revise_count;
                target_entry.leech = source_entry.leech;
                updated += 1;
            }
        }
    }

    if updated > 0 {
        db::write_db(target_path, &target_db)?;
    }

    Ok(updated)
}

fn main() -> Result<()> {
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();
    let args = Args::parse();

    let vault = resolve_vault(&args);
    vault
        .ensure_dir()
        .context("Failed to create .carddown/ directory")?;

    log::debug!("Vault root: {}", vault.root.display());

    // Acquire lock file with proper RAII cleanup
    let _lock_guard = if args.force {
        LockGuard::force_new(&vault.lock_file)
            .context("Failed to acquire lock file (force mode)")?
    } else {
        LockGuard::new(&vault.lock_file)
            .context("Failed to acquire lock file. Another instance may be running. Use --force to remove the lock file.")?
    };

    match args.command {
        Commands::Scan {
            file_types,
            full,
            path,
        } => {
            let all_cards = if path.is_dir() {
                let file_types_set = file_types_to_set(&file_types);
                let mut index = load_scan_index(&vault.scan_index_file);
                let files = collect_files(&path, &file_types_set);
                let to_scan: Vec<PathBuf> = if full {
                    files
                } else {
                    let mut modified = Vec::new();
                    for f in files.iter() {
                        let m = mtime_secs(f).unwrap_or(0);
                        let key = f.to_string_lossy().to_string();
                        if index.get(&key).copied().unwrap_or(0) < m {
                            modified.push(f.clone());
                        }
                        index.insert(key, m);
                    }
                    if modified.is_empty() {
                        log::info!("No modified files detected; skipping scan");
                        save_scan_index(&vault.scan_index_file, &index);
                        return Ok(());
                    }
                    modified
                };
                let mut acc: Vec<Card> = Vec::new();
                for f in &to_scan {
                    let mut cs = card::parse_file(f)?;
                    acc.append(&mut cs);
                }
                save_scan_index(&vault.scan_index_file, &index);
                acc
            } else if path.is_file() {
                let mut index = load_scan_index(&vault.scan_index_file);
                let m = mtime_secs(&path).unwrap_or(0);
                index.insert(path.to_string_lossy().to_string(), m);
                save_scan_index(&vault.scan_index_file, &index);
                card::parse_file(&path)?
            } else {
                vec![]
            };
            let stats = db::update_db(&vault.db_file, all_cards, full)?;
            let mut parts = vec![format!("Found {} card(s)", stats.found)];
            if stats.new > 0 {
                parts.push(format!("{} new", stats.new));
            }
            if stats.updated > 0 {
                parts.push(format!("{} updated", stats.updated));
            }
            if stats.orphaned > 0 {
                parts.push(format!("{} orphaned", stats.orphaned));
            }
            if stats.unorphaned > 0 {
                parts.push(format!("{} restored", stats.unorphaned));
            }
            println!("{}", parts.join(", "));
        }
        Commands::Audit {} => {
            let db = db::get_db(&vault.db_file)?;
            let db_file = vault.db_file.clone();
            let cards = db.into_values().filter(|c| c.orphan || c.leech).collect();
            let mut terminal = view::init()?;
            let res =
                view::audit::App::new(cards, Box::new(move |id| db::delete_card(&db_file, id)))
                    .run(&mut terminal);
            view::restore()?;
            res?
        }
        Commands::Revise {
            algorithm,
            cram,
            cram_hours,
            include_orphans,
            leech_failure_threshold,
            leech_method,
            maximum_cards_per_session,
            maximum_duration_of_session,
            reverse_probability,
            tag: tags,
        } => {
            let db = db::get_db(&vault.db_file)?;
            let mut state = db::get_global_state(&vault.state_file)?;
            db::refresh_global_state(&mut state);
            let tags_set: HashSet<String> = tags.iter().cloned().collect();
            let mut cards = filter_cards(
                db,
                tags_set,
                include_orphans,
                leech_method,
                cram,
                cram_hours,
            );
            cards.shuffle(&mut rand::rng());
            let cards: Vec<_> = cards.into_iter().take(maximum_cards_per_session).collect();
            if cards.is_empty() {
                println!("No cards due for review.");
                return Ok(());
            }
            println!("{} card(s) due for review.", cards.len());
            let total_cards = cards.len();
            let mut terminal = view::init()?;
            let db_file = vault.db_file.clone();
            let state_file = vault.state_file.clone();
            let mut app = view::revise::App::new(
                new_algorithm(algorithm),
                cards,
                state,
                view::revise::ReviseConfig {
                    leech_threshold: leech_failure_threshold,
                    max_duration: maximum_duration_of_session,
                    reverse_probability,
                    tags,
                },
                Box::new(move |cards, state| {
                    if !cram {
                        let _ = db::update_cards(&db_file, cards);
                        db::write_global_state(&state_file, state)?;
                    }
                    Ok(())
                }),
            );
            let res = app.run(&mut terminal);
            let reviewed = app.cards_reviewed();
            view::restore()?;
            res?;
            println!("Reviewed {reviewed}/{total_cards} card(s).")
        }
        Commands::Import { source } => {
            if !source.exists() {
                anyhow::bail!("Source file not found: {}", source.display());
            }
            let updated = import_stats(&source, &vault.db_file)?;
            println!(
                "Imported review history for {updated} card(s) into {}",
                vault.db_file.display()
            );
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_cards_from_folder() {
        let folder = PathBuf::from("tests/fixtures");
        let file_types = vec!["md".to_string()];
        let cards = parse_cards_from_folder(&folder, &file_types).unwrap();
        // 2 single-line + 2 multi-line = 4 (ignored.md is skipped)
        assert_eq!(cards.len(), 4);
        assert!(cards.iter().any(|c| c.prompt == "Capital of France?"));
        assert!(cards.iter().any(|c| c.prompt == "Explain photosynthesis"));
        // Verify ignored file was skipped
        assert!(!cards.iter().any(|c| c.prompt.contains("ignored")));
    }

    #[test]
    fn test_parse_cards_from_folder_type_filter() {
        let folder = PathBuf::from("tests/fixtures");
        let file_types = vec!["txt".to_string()];
        let cards = parse_cards_from_folder(&folder, &file_types).unwrap();
        assert!(cards.is_empty());
    }

    fn get_card_db() -> CardDb {
        let mut db = CardDb::new();
        let card = Card {
            id: blake3::hash(b"test"),
            file: PathBuf::from("tests/test.md"),
            line: 0,
            prompt: "What is the answer to life, the universe, and everything?".to_string(),
            response: vec!["42".to_string()],
            tags: HashSet::from(["card".to_string()]),
        };
        let entry = CardEntry::new(card);
        db.insert(entry.card.id, entry);
        db
    }

    fn run_filter(
        db: CardDb,
        tags: HashSet<String>,
        orphans: bool,
        leech: LeechMethod,
        cram: bool,
        cram_hours: usize,
    ) -> Vec<CardEntry> {
        filter_cards(db, tags, orphans, leech, cram, cram_hours)
    }

    fn run_filter_defaults(db: CardDb) -> Vec<CardEntry> {
        run_filter(db, HashSet::new(), false, LeechMethod::Skip, false, 12)
    }

    #[test]
    fn test_filter_cards_empty() {
        let cards = run_filter_defaults(CardDb::new());
        assert!(cards.is_empty());
    }

    #[test]
    fn test_filter_cards_zero_interval() {
        let mut db = get_card_db();
        db.get_mut(&blake3::hash(b"test")).unwrap().state.interval = 0;
        assert_eq!(run_filter_defaults(db).len(), 1);
    }

    #[test]
    fn test_filter_cards_interval_last_viewed_none() {
        let mut db = get_card_db();
        db.get_mut(&blake3::hash(b"test")).unwrap().state.interval = 1;
        assert_eq!(run_filter_defaults(db).len(), 1);
    }

    #[test]
    fn test_filter_cards_interval() {
        let mut db = get_card_db();
        let entry = db.get_mut(&blake3::hash(b"test")).unwrap();
        entry.state.interval = 1;
        entry.last_revised = Some(chrono::Utc::now());
        assert!(run_filter_defaults(db).is_empty());
    }

    #[test]
    fn test_filter_cards_lapsed_interval() {
        let mut db = get_card_db();
        let entry = db.get_mut(&blake3::hash(b"test")).unwrap();
        entry.state.interval = 1;
        entry.last_revised = Some(chrono::Utc::now() - chrono::Duration::days(1));
        assert_eq!(run_filter_defaults(db).len(), 1);
    }

    #[test]
    fn test_filter_cards_cram_mode() {
        let mut db = get_card_db();
        let entry = db.get_mut(&blake3::hash(b"test")).unwrap();
        entry.last_revised = Some(chrono::Utc::now() - chrono::Duration::hours(13));
        entry.state.interval = 2;
        assert_eq!(
            run_filter(db, HashSet::new(), false, LeechMethod::Skip, true, 12).len(),
            1
        );

        let mut db = get_card_db();
        let entry = db.get_mut(&blake3::hash(b"test")).unwrap();
        entry.last_revised = Some(chrono::Utc::now() - chrono::Duration::hours(11));
        entry.state.interval = 2;
        assert!(run_filter(db, HashSet::new(), false, LeechMethod::Skip, true, 12).is_empty());

        let mut db = get_card_db();
        let entry = db.get_mut(&blake3::hash(b"test")).unwrap();
        entry.last_revised = Some(chrono::Utc::now());
        entry.state.interval = 2;
        assert_eq!(
            run_filter(db, HashSet::new(), false, LeechMethod::Skip, true, 0).len(),
            1
        );
    }

    #[test]
    fn test_filter_cards_matching_tags() {
        let tags = HashSet::from(["card".to_string(), "test".to_string()]);
        assert_eq!(
            run_filter(get_card_db(), tags, false, LeechMethod::Skip, false, 12).len(),
            1
        );
    }

    #[test]
    fn test_filter_cards_non_matching_tags() {
        let tags = HashSet::from(["foo".to_string(), "test".to_string()]);
        assert!(run_filter(get_card_db(), tags, false, LeechMethod::Skip, false, 12).is_empty());
    }

    #[test]
    fn test_filter_cards_orphans() {
        let mut db = get_card_db();
        db.get_mut(&blake3::hash(b"test")).unwrap().orphan = true;
        assert!(run_filter_defaults(db).is_empty());
    }

    #[test]
    fn test_filter_cards_skip_leech() {
        let mut db = get_card_db();
        db.get_mut(&blake3::hash(b"test")).unwrap().leech = true;
        assert!(run_filter_defaults(db).is_empty());
    }

    #[test]
    fn test_filter_cards_warn_leech() {
        let mut db = get_card_db();
        db.get_mut(&blake3::hash(b"test")).unwrap().leech = true;
        assert_eq!(
            run_filter(db, HashSet::new(), false, LeechMethod::Warn, false, 12).len(),
            1
        );
    }

    #[test]
    fn test_filter_cards_include_orphans() {
        let mut db = get_card_db();
        db.get_mut(&blake3::hash(b"test")).unwrap().orphan = true;
        assert_eq!(
            run_filter(db, HashSet::new(), true, LeechMethod::Skip, false, 12).len(),
            1
        );
    }

    #[test]
    fn test_filter_cards_exact_cram_boundary() {
        let mut db = get_card_db();
        let entry = db.get_mut(&blake3::hash(b"test")).unwrap();
        entry.last_revised = Some(chrono::Utc::now() - chrono::Duration::hours(12));
        assert_eq!(
            run_filter(db, HashSet::new(), false, LeechMethod::Skip, true, 12).len(),
            1
        );
    }

    #[test]
    fn test_filter_cards_multiple_matching_tags() {
        let mut db = get_card_db();
        db.get_mut(&blake3::hash(b"test"))
            .unwrap()
            .card
            .tags
            .insert("extra_tag".to_string());
        let tags = HashSet::from(["card".to_string(), "extra_tag".to_string()]);
        let cards = run_filter(db, tags, false, LeechMethod::Skip, false, 12);
        assert_eq!(cards.len(), 1);
    }

    #[test]
    fn test_import_stats() {
        use tempfile::NamedTempFile;

        let source_file = NamedTempFile::new().unwrap();
        let target_file = NamedTempFile::new().unwrap();

        // Create source db with reviewed card
        let mut source_db = get_card_db();
        let entry = source_db.get_mut(&blake3::hash(b"test")).unwrap();
        entry.revise_count = 10;
        entry.last_revised = Some(chrono::Utc::now());
        entry.state.interval = 5;
        db::write_db(source_file.path(), &source_db).unwrap();

        // Create target db with same card but no reviews
        let target_db = get_card_db();
        db::write_db(target_file.path(), &target_db).unwrap();

        let updated = import_stats(source_file.path(), target_file.path()).unwrap();
        assert_eq!(updated, 1);

        // Verify stats were imported
        let result_db = db::get_db(target_file.path()).unwrap();
        let card = result_db.get(&blake3::hash(b"test")).unwrap();
        assert_eq!(card.revise_count, 10);
        assert_eq!(card.state.interval, 5);
    }

    #[test]
    fn test_import_stats_skips_lower_count() {
        use tempfile::NamedTempFile;

        let source_file = NamedTempFile::new().unwrap();
        let target_file = NamedTempFile::new().unwrap();

        // Source has fewer reviews
        let mut source_db = get_card_db();
        source_db
            .get_mut(&blake3::hash(b"test"))
            .unwrap()
            .revise_count = 2;
        db::write_db(source_file.path(), &source_db).unwrap();

        // Target has more reviews
        let mut target_db = get_card_db();
        target_db
            .get_mut(&blake3::hash(b"test"))
            .unwrap()
            .revise_count = 5;
        db::write_db(target_file.path(), &target_db).unwrap();

        let updated = import_stats(source_file.path(), target_file.path()).unwrap();
        assert_eq!(updated, 0);

        // Verify target was not modified
        let result_db = db::get_db(target_file.path()).unwrap();
        assert_eq!(
            result_db.get(&blake3::hash(b"test")).unwrap().revise_count,
            5
        );
    }
}
