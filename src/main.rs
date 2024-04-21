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
use std::collections::HashSet;
use std::path::PathBuf;
use walkdir::WalkDir;

#[macro_use]
extern crate lazy_static;

lazy_static! {
    static ref DB_PATH: String = format!(
        "{}/carddown",
        std::env::var("XDG_STATE_HOME").unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap();
            format!("{}/.local/state", home)
        })
    );
    static ref DB_FILE_PATH: String = format!("{}/cards.ron", *DB_PATH);
    static ref STATE_FILE_PATH: String = format!("{}/state.ron", *DB_PATH);
}

#[derive(Debug, Clone, ValueEnum)]
enum LeechMethod {
    Skip,
    Warn,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Scan file or folder for cards
    Scan {
        /// File types to parse
        #[arg(long, default_values_t = ["md".to_string(), "txt".to_string(), "org".to_string()])]
        file_types: Vec<String>,

        /// Full scan (default incremental), can generate orphans
        #[arg(long)]
        full: bool,

        /// Path to file or folder to scan
        path: PathBuf,
    },
    /// Audit the card database for orphaned and leech cards
    Audit {},
    /// Revise pending flahscards
    Revise {
        #[arg(long, default_value_t = 30)]
        maximum_cards_per_session: usize,

        /// in minutes
        #[arg(long, default_value_t = 20)]
        maximum_duration_of_session: usize,

        /// Threshold before a item is defined as a leech.
        #[arg(long, default_value_t = 15)]
        leech_failure_threshold: usize,

        #[arg(long, value_enum, default_value_t = LeechMethod::Skip)]
        leech_method: LeechMethod,

        #[arg(long, value_enum, default_value_t = Algo::SM5)]
        algorithm: Algo,

        /// Tags to filter cards, no tags matches all cards
        #[arg(long)]
        tag: Vec<String>,

        /// include orphaned cards
        #[arg(long)]
        include_orphans: bool,

        /// Likelihood that prompt and response are swapped.
        /// 0 = never, 1 = always
        #[arg(long, default_value_t = 0.0)]
        reverse_probability: f64,

        /// Cram session. Revise all cards regardless of interval if they haven't been revised
        /// in the last 12 hours. Does not effect spaced repetition stats of the cards.
        #[arg(long)]
        cram: bool,
    },
}

/// CARDDOWN is a simple cli tool to keep track of (and study) flashcards in text files.
#[derive(Parser, Debug)]
#[command(version, about, long_about=None)]
struct Args {
    #[command(subcommand)]
    command: Commands,

    /// Path to the database file
    #[arg(long, default_value = &**DB_FILE_PATH)]
    db: PathBuf,

    /// Path to the state file
    #[arg(long, default_value = &**STATE_FILE_PATH)]
    state: PathBuf,
}

// walk file tree and parse all files
fn parse_cards_from_folder(folder: &PathBuf, file_types: &[String]) -> Result<Vec<Card>> {
    let file_types: HashSet<&str> = HashSet::from_iter(file_types.iter().map(|s| s.as_str()));
    WalkDir::new(folder)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .filter(|e| {
            if let Some(ft) = e.path().extension() {
                file_types.contains(ft.to_string_lossy().as_ref())
            } else {
                false
            }
        })
        .map(|e| card::parse_file(&PathBuf::from(e.path())))
        .collect::<Result<Vec<Vec<Card>>>>()
        .map(|xs| xs.into_iter().flatten().collect())
}

fn filter_cards(
    db: CardDb,
    tags: HashSet<String>,
    include_orphans: bool,
    leech_method: LeechMethod,
    cram_mode: bool,
) -> Vec<CardEntry> {
    let today = chrono::Utc::now();
    db.into_values()
        .filter(|c| {
            if let Some(last_revised) = c.last_revised {
                if cram_mode {
                    today - last_revised > chrono::Duration::hours(12)
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
    if !PathBuf::from(&**DB_PATH).exists() {
        std::fs::create_dir_all(&**DB_PATH)?;
    }

    let args = Args::parse();
    match args.command {
        Commands::Scan {
            file_types,
            full,
            path,
        } => {
            let all_cards = if path.is_dir() {
                parse_cards_from_folder(&path, &file_types)?
            } else if path.is_file() {
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
            let tags_set: HashSet<String> = HashSet::from_iter(tags.iter().cloned());
            let mut cards = filter_cards(db, tags_set, include_orphans, leech_method, cram);
            cards.shuffle(&mut rand::thread_rng());
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
            response: "42".to_string(),
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
        let leech_method = LeechMethod::Skip;
        let cards = filter_cards(db, tags, include_orphans, leech_method, cram_mode);
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
        let cards = filter_cards(db, tags, include_orphans, leech_method, cram_mode);
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
        let cards = filter_cards(db, tags, include_orphans, leech_method, cram_mode);
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
        let cards = filter_cards(db, tags, include_orphans, leech_method, cram_mode);
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
        let cards = filter_cards(db, tags, include_orphans, leech_method, cram_mode);
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
        let cards = filter_cards(db, tags, include_orphans, leech_method, cram_mode);
        assert_eq!(cards.len(), 1);
    }

    #[test]
    fn test_filter_cards_matching_tags() {
        let db = get_card_db();
        let tags = HashSet::from_iter(vec!["card".to_string(), "test".to_string()]);
        let include_orphans = false;
        let leech_method = LeechMethod::Skip;
        let cram_mode = false;
        let cards = filter_cards(db, tags, include_orphans, leech_method, cram_mode);
        assert_eq!(cards.len(), 1);
    }

    #[test]
    fn test_filter_cards_non_matching_tags() {
        let db = get_card_db();
        let tags = HashSet::from_iter(vec!["foo".to_string(), "test".to_string()]);
        let include_orphans = false;
        let leech_method = LeechMethod::Skip;
        let cram_mode = false;
        let cards = filter_cards(db, tags, include_orphans, leech_method, cram_mode);
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
        let cards = filter_cards(db, tags, include_orphans, leech_method, cram_mode);
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
        let cards = filter_cards(db, tags, include_orphans, leech_method, cram_mode);
        assert!(cards.is_empty());
    }
}
