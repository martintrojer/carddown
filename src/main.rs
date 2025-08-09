mod algorithm;
mod card;
mod db;
mod view;

use crate::algorithm::Algo;
use crate::card::Card;
use crate::db::CardDb;
use crate::db::CardEntry;
use algorithm::new_algorithm;
use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};
use env_logger::Env;
use rand::prelude::*;
use std::collections::{HashMap, HashSet};
use std::fs::OpenOptions;
use std::io::ErrorKind;
use std::path::PathBuf;
use std::time::UNIX_EPOCH;
use walkdir::WalkDir;

use std::sync::LazyLock;

/// RAII guard for lock file management
struct LockGuard;

impl LockGuard {
    fn new() -> Result<Self> {
        // Use atomic create-new operation to prevent race conditions
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&*LOCK_FILE_PATH)
        {
            Ok(_) => Ok(LockGuard),
            Err(e) if e.kind() == ErrorKind::AlreadyExists => {
                anyhow::bail!("Another instance of carddown is running, or the previous instance crashed. Use --force to remove the lock file.");
            }
            Err(e) => Err(e.into()),
        }
    }

    fn force_new() -> Result<Self> {
        // Remove existing lock file if it exists
        let _ = std::fs::remove_file(&*LOCK_FILE_PATH);
        std::fs::File::create(&*LOCK_FILE_PATH)?;
        Ok(LockGuard)
    }
}

impl Drop for LockGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&*LOCK_FILE_PATH);
    }
}

static DB_PATH: LazyLock<String> = LazyLock::new(|| {
    format!(
        "{}/carddown",
        std::env::var("XDG_STATE_HOME").unwrap_or_else(|_| {
            std::env::var("HOME")
                .map(|home| format!("{home}/.local/state"))
                .unwrap_or_else(|_| format!("{}/.local/state", std::env::temp_dir().display()))
        })
    )
});
static DB_FILE_PATH: LazyLock<String> = LazyLock::new(|| format!("{}/cards.json", &*DB_PATH));
static STATE_FILE_PATH: LazyLock<String> = LazyLock::new(|| format!("{}/state.json", &*DB_PATH));
static LOCK_FILE_PATH: LazyLock<String> = LazyLock::new(|| format!("{}/lock", &*DB_PATH));
static SCAN_INDEX_FILE_PATH: LazyLock<String> =
    LazyLock::new(|| format!("{}/scan_index.json", &*DB_PATH));

type ScanIndex = HashMap<String, u64>; // file path -> mtime seconds

fn load_scan_index() -> ScanIndex {
    let path = PathBuf::from(&*SCAN_INDEX_FILE_PATH);
    if let Ok(data) = std::fs::read_to_string(&path) {
        serde_json::from_str(&data).unwrap_or_default()
    } else {
        HashMap::new()
    }
}

fn save_scan_index(index: &ScanIndex) {
    if let Ok(json) = serde_json::to_string(index) {
        let _ = std::fs::write(&*SCAN_INDEX_FILE_PATH, json);
    }
}

#[derive(Debug, Clone, ValueEnum)]
enum LeechMethod {
    Skip,
    Warn,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Scan files for flashcards and add them to the database
    Scan {
        /// File extensions to scan (e.g., md, txt, org)
        #[arg(long, default_values_t = ["md".to_string(), "txt".to_string(), "org".to_string()])]
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
        #[arg(long, default_value_t = 30)]
        maximum_cards_per_session: usize,

        /// Maximum length of review session in minutes
        #[arg(long, default_value_t = 20)]
        maximum_duration_of_session: usize,

        /// Number of failures before a card is marked as a leech
        #[arg(long, default_value_t = 15)]
        leech_failure_threshold: usize,

        /// How to handle leech cards during review:
        /// skip - Skip leech cards entirely.
        /// warn - Show leech cards but display a warning
        #[arg(long, value_enum, default_value_t = LeechMethod::Skip)]
        leech_method: LeechMethod,

        /// Spaced repetition algorithm to determine card intervals
        #[arg(long, value_enum, default_value_t = Algo::SM5)]
        algorithm: Algo,

        /// Only show cards with these tags (shows all cards if no tags specified)
        #[arg(long)]
        tag: Vec<String>,

        /// Include cards whose source files no longer exist
        #[arg(long)]
        include_orphans: bool,

        /// Chance to swap question/answer (0.0 = never, 1.0 = always)
        #[arg(long, default_value_t = 0.0)]
        reverse_probability: f64,

        /// Enable review of all cards not seen in --cram-hours, ignoring intervals
        /// Note: Reviews in cram mode don't affect card statistics
        #[arg(long)]
        cram: bool,

        /// Hours since last review for cards to include in cram mode
        #[arg(long, default_value_t = 12)]
        cram_hours: usize,
    },
}

/// CARDDOWN - A command-line flashcard system that manages cards from text files
/// Cards are extracted from text files and can be reviewed using spaced repetition.
/// The system tracks review history and automatically schedules cards for optimal learning.
#[derive(Parser, Debug)]
#[command(version, about, long_about=None)]
struct Args {
    #[command(subcommand)]
    command: Commands,

    /// Location of the card database file
    #[arg(long, default_value = &**DB_FILE_PATH)]
    db: PathBuf,

    /// Location of the program state file
    #[arg(long, default_value = &**STATE_FILE_PATH)]
    state: PathBuf,

    /// Override file locking mechanism.
    /// Warning: Only use if no other carddown instances are running
    #[arg(long)]
    force: bool,
}

// walk file tree and parse all files
#[allow(dead_code)]
fn parse_cards_from_folder(folder: &PathBuf, file_types: &[String]) -> Result<Vec<Card>> {
    let file_types: HashSet<&str> = HashSet::from_iter(file_types.iter().map(|s| s.as_str()));
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
        .try_fold(Vec::new(), |mut acc, e| {
            let mut cards = card::parse_file(e.path())?;
            acc.append(&mut cards);
            Ok(acc)
        })
}

fn mtime_secs(path: &std::path::Path) -> Option<u64> {
    std::fs::metadata(path)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
}

fn collect_files(folder: &PathBuf, file_types: &HashSet<&str>) -> Vec<PathBuf> {
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
        .filter(|c| {
            if let Some(last_revised) = c.last_revised {
                if cram_mode {
                    today - last_revised >= chrono::Duration::hours(cram_hours as i64)
                } else {
                    let next_date = last_revised + chrono::Duration::days(c.state.interval as i64);
                    today >= next_date
                }
            } else {
                true
            }
        })
        .filter(|c| tags.is_empty() || c.card.tags.intersection(&tags).count() > 0)
        .filter(|c| include_orphans || !c.orphan)
        .filter(|c| !(matches!(leech_method, LeechMethod::Skip) && c.leech))
        .collect()
}

fn main() -> Result<()> {
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();
    let args = Args::parse();

    if !PathBuf::from(&*DB_PATH).exists() {
        std::fs::create_dir_all(&*DB_PATH)?;
    }

    // Acquire lock file with proper RAII cleanup
    let _lock_guard = if args.force {
        LockGuard::force_new()?
    } else {
        match LockGuard::new() {
            Ok(guard) => guard,
            Err(_) => {
                log::error!("Another instance of carddown is running, or the previous instance crashed. Use --force to remove the lock file.");
                std::process::exit(1);
            }
        }
    };

    match args.command {
        Commands::Scan {
            file_types,
            full,
            path,
        } => {
            let all_cards = if path.is_dir() {
                let file_types_set: HashSet<&str> =
                    HashSet::from_iter(file_types.iter().map(|s| s.as_str()));
                let mut index = load_scan_index();
                let files = collect_files(&path, &file_types_set);
                let mut to_scan: Vec<PathBuf> = Vec::new();
                if full {
                    to_scan = files.clone();
                } else {
                    for f in files.iter() {
                        let m = mtime_secs(f).unwrap_or(0);
                        let key = f.to_string_lossy().to_string();
                        if index.get(&key).copied().unwrap_or(0) < m {
                            to_scan.push(f.clone());
                        }
                        // Update index with current mtime so next run can skip
                        index.insert(key, m);
                    }
                }
                // If not full and nothing changed, short-circuit
                if !full && to_scan.is_empty() {
                    log::info!("No modified files detected; skipping scan");
                    return Ok(());
                }
                // Parse selected files
                let mut acc: Vec<Card> = Vec::new();
                for f in if full { files } else { to_scan } {
                    let mut cs = card::parse_file(&f)?;
                    acc.append(&mut cs);
                }
                // Save index for future incremental scans
                save_scan_index(&index);
                acc
            } else if path.is_file() {
                // Single file; update index for this file
                let mut index = load_scan_index();
                let m = mtime_secs(&path).unwrap_or(0);
                index.insert(path.to_string_lossy().to_string(), m);
                save_scan_index(&index);
                card::parse_file(&path)?
            } else {
                vec![]
            };
            db::update_db(&args.db, all_cards, full)?;
        }
        Commands::Audit {} => {
            let db = db::get_db(&args.db)?;
            let cards = db.into_values().filter(|c| c.orphan || c.leech).collect();
            let mut terminal = view::init()?;
            let res =
                view::audit::App::new(cards, Box::new(move |id| db::delete_card(&args.db, id)))
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
            let db = db::get_db(&args.db)?;
            let mut state = db::get_global_state(&args.state)?;
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
            let mut terminal = view::init()?;
            let res = view::revise::App::new(
                new_algorithm(algorithm),
                cards,
                state,
                leech_failure_threshold,
                maximum_duration_of_session,
                reverse_probability,
                tags,
                Box::new(move |cards, state| {
                    // Dont update the database if we are in cram mode
                    if !cram {
                        let _ = db::update_cards(&args.db, cards);
                        db::write_global_state(&args.state, state)?;
                    }
                    Ok(())
                }),
            )
            .run(&mut terminal);
            view::restore()?;
            res?
        }
    }

    // Lock file will be automatically cleaned up when _lock_guard goes out of scope
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_cards_from_folder() {
        let folder = PathBuf::from("tests");
        let file_types = vec!["md".to_string()];
        let cards = parse_cards_from_folder(&folder, &file_types).unwrap();
        assert_eq!(cards.len(), 4);
    }

    #[test]
    fn test_parse_cards_from_folder_type_filter() {
        let folder = PathBuf::from("tests");
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

    #[test]
    fn test_filter_cards_empty() {
        let db = CardDb::new();
        let tags = HashSet::new();
        let include_orphans = false;
        let cram_mode = false;
        let cram_hours = 12;
        let leech_method = LeechMethod::Skip;
        let cards = filter_cards(
            db,
            tags,
            include_orphans,
            leech_method,
            cram_mode,
            cram_hours,
        );
        assert_eq!(cards.len(), 0);
    }

    #[test]
    fn test_filter_cards_zero_interval() {
        let mut db = get_card_db();
        let entry = db.get_mut(&blake3::hash(b"test")).unwrap();
        entry.state.interval = 0;
        let tags = HashSet::new();
        let include_orphans = false;
        let leech_method = LeechMethod::Skip;
        let cram_mode = false;
        let cram_hours = 12;
        let cards = filter_cards(
            db,
            tags,
            include_orphans,
            leech_method,
            cram_mode,
            cram_hours,
        );
        assert_eq!(cards.len(), 1);
    }

    #[test]
    fn test_filter_cards_interval_last_viewed_none() {
        let mut db = get_card_db();
        let entry = db.get_mut(&blake3::hash(b"test")).unwrap();
        entry.state.interval = 1;
        let tags = HashSet::new();
        let include_orphans = false;
        let leech_method = LeechMethod::Skip;
        let cram_mode = false;
        let cram_hours = 12;
        let cards = filter_cards(
            db,
            tags,
            include_orphans,
            leech_method,
            cram_mode,
            cram_hours,
        );
        assert_eq!(cards.len(), 1);
    }

    #[test]
    fn test_filter_cards_interval() {
        let mut db = get_card_db();
        let entry = db.get_mut(&blake3::hash(b"test")).unwrap();
        entry.state.interval = 1;
        entry.last_revised = Some(chrono::Utc::now());
        let tags = HashSet::new();
        let include_orphans = false;
        let leech_method = LeechMethod::Skip;
        let cram_mode = false;
        let cram_hours = 12;
        let cards = filter_cards(
            db,
            tags,
            include_orphans,
            leech_method,
            cram_mode,
            cram_hours,
        );
        assert!(cards.is_empty());
    }

    #[test]
    fn test_filter_cards_lapsed_interval() {
        let mut db = get_card_db();
        let entry = db.get_mut(&blake3::hash(b"test")).unwrap();
        entry.state.interval = 1;
        entry.last_revised = Some(chrono::Utc::now() - chrono::Duration::days(1));
        let tags = HashSet::new();
        let include_orphans = false;
        let leech_method = LeechMethod::Skip;
        let cram_mode = false;
        let cram_hours = 12;
        let cards = filter_cards(
            db,
            tags,
            include_orphans,
            leech_method,
            cram_mode,
            cram_hours,
        );
        assert_eq!(cards.len(), 1);
    }

    #[test]
    fn test_filter_cards_cram_mode() {
        let mut db = get_card_db();
        let entry = db.get_mut(&blake3::hash(b"test")).unwrap();
        entry.last_revised = Some(chrono::Utc::now() - chrono::Duration::hours(13));
        entry.state.interval = 2;
        let tags = HashSet::new();
        let include_orphans = false;
        let leech_method = LeechMethod::Skip;
        let cram_mode = true;
        let cram_hours = 12;
        let cards = filter_cards(
            db,
            tags.clone(),
            include_orphans,
            leech_method.clone(),
            cram_mode,
            cram_hours,
        );
        assert_eq!(cards.len(), 1);

        let mut db = get_card_db();
        let entry = db.get_mut(&blake3::hash(b"test")).unwrap();
        entry.last_revised = Some(chrono::Utc::now() - chrono::Duration::hours(11));
        entry.state.interval = 2;
        let cards = filter_cards(
            db,
            tags.clone(),
            include_orphans,
            leech_method.clone(),
            cram_mode,
            cram_hours,
        );
        assert!(cards.is_empty());

        let mut db = get_card_db();
        let entry = db.get_mut(&blake3::hash(b"test")).unwrap();
        entry.last_revised = Some(chrono::Utc::now());
        entry.state.interval = 2;
        let cards = filter_cards(db, tags, include_orphans, leech_method, cram_mode, 0);
        assert_eq!(cards.len(), 1);
    }

    #[test]
    fn test_filter_cards_matching_tags() {
        let db = get_card_db();
        let tags = HashSet::from_iter(vec!["card".to_string(), "test".to_string()]);
        let include_orphans = false;
        let leech_method = LeechMethod::Skip;
        let cram_mode = false;
        let cram_hours = 12;
        let cards = filter_cards(
            db,
            tags,
            include_orphans,
            leech_method,
            cram_mode,
            cram_hours,
        );
        assert_eq!(cards.len(), 1);
    }

    #[test]
    fn test_filter_cards_non_matching_tags() {
        let db = get_card_db();
        let tags = HashSet::from_iter(vec!["foo".to_string(), "test".to_string()]);
        let include_orphans = false;
        let leech_method = LeechMethod::Skip;
        let cram_mode = false;
        let cram_hours = 12;
        let cards = filter_cards(
            db,
            tags,
            include_orphans,
            leech_method,
            cram_mode,
            cram_hours,
        );
        assert!(cards.is_empty());
    }

    #[test]
    fn test_filter_cards_orphans() {
        let mut db = get_card_db();
        let entry = db.get_mut(&blake3::hash(b"test")).unwrap();
        entry.orphan = true;
        let tags = HashSet::new();
        let include_orphans = false;
        let leech_method = LeechMethod::Skip;
        let cram_mode = false;
        let cram_hours = 12;
        let cards = filter_cards(
            db,
            tags,
            include_orphans,
            leech_method,
            cram_mode,
            cram_hours,
        );
        assert!(cards.is_empty());
    }

    #[test]
    fn test_filter_cards_skip_leech() {
        let mut db = get_card_db();
        let entry = db.get_mut(&blake3::hash(b"test")).unwrap();
        entry.leech = true;
        let tags = HashSet::new();
        let include_orphans = false;
        let leech_method = LeechMethod::Skip;
        let cram_mode = false;
        let cram_hours = 12;
        let cards = filter_cards(
            db,
            tags,
            include_orphans,
            leech_method,
            cram_mode,
            cram_hours,
        );
        assert!(cards.is_empty());
    }

    #[test]
    fn test_filter_cards_warn_leech() {
        let mut db = get_card_db();
        let entry = db.get_mut(&blake3::hash(b"test")).unwrap();
        entry.leech = true;
        let tags = HashSet::new();
        let include_orphans = false;
        let leech_method = LeechMethod::Warn;
        let cram_mode = false;
        let cram_hours = 12;
        let cards = filter_cards(
            db,
            tags,
            include_orphans,
            leech_method,
            cram_mode,
            cram_hours,
        );
        assert_eq!(cards.len(), 1); // LeechMethod::Warn should include leech cards
    }

    #[test]
    fn test_filter_cards_include_orphans() {
        let mut db = get_card_db();
        let entry = db.get_mut(&blake3::hash(b"test")).unwrap();
        entry.orphan = true;
        let tags = HashSet::new();
        let include_orphans = true;
        let leech_method = LeechMethod::Skip;
        let cram_mode = false;
        let cram_hours = 12;
        let cards = filter_cards(
            db,
            tags,
            include_orphans,
            leech_method,
            cram_mode,
            cram_hours,
        );
        assert_eq!(cards.len(), 1); // Should include orphaned cards when include_orphans is true
    }

    #[test]
    fn test_filter_cards_exact_cram_boundary() {
        let mut db = get_card_db();
        let entry = db.get_mut(&blake3::hash(b"test")).unwrap();
        entry.last_revised = Some(chrono::Utc::now() - chrono::Duration::hours(12));
        let tags = HashSet::new();
        let include_orphans = false;
        let leech_method = LeechMethod::Skip;
        let cram_mode = true;
        let cram_hours = 12;
        let cards = filter_cards(
            db,
            tags,
            include_orphans,
            leech_method,
            cram_mode,
            cram_hours,
        );
        assert_eq!(cards.len(), 1); // Should include cards exactly at the cram_hours boundary
    }

    #[test]
    fn test_filter_cards_multiple_matching_tags() {
        let mut db = get_card_db();
        let entry = db.get_mut(&blake3::hash(b"test")).unwrap();
        entry.card.tags.insert("extra_tag".to_string());
        let tags = HashSet::from_iter(vec!["card".to_string(), "extra_tag".to_string()]);
        let include_orphans = false;
        let leech_method = LeechMethod::Skip;
        let cram_mode = false;
        let cram_hours = 12;
        let cards = filter_cards(
            db,
            tags,
            include_orphans,
            leech_method,
            cram_mode,
            cram_hours,
        );
        assert_eq!(cards.len(), 1); // Should match when card has multiple matching tags
    }
}
