mod algorithm;
mod card;
mod db;
mod view;

use crate::algorithm::Algo;
use crate::card::Card;
use crate::db::CardDb;
use crate::db::CardEntry;
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
    static ref DB_FILE_PATH: String = format!("{}/cards.json", *DB_PATH);
    static ref STATE_FILE_PATH: String = format!("{}/state.json", *DB_PATH);
}

#[derive(Debug, Clone, ValueEnum)]
enum LeechMethod {
    Skip,
    Warn,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Scan file of folder for cards
    Scan {
        /// Use a single file as input
        #[arg(long)]
        file: Option<PathBuf>,

        /// Walk a directory and use all files as input
        #[arg(long, conflicts_with("file"))]
        folder: Option<PathBuf>,

        /// File types to parse
        #[arg(long, default_values_t = ["md".to_string(), "txt".to_string(), "org".to_string()])]
        file_types: Vec<String>,

        /// Full scan (different from default incremental), will generate orphans if found
        #[arg(long)]
        full: bool,
    },
    /// Audit the card database for orphaned and leech cards
    Audit {},
    /// Run a revise session
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

        #[arg(long, value_enum, default_value_t = Algo::SM2)]
        algorithm: Algo,

        /// Tags to filter cards
        #[arg(long)]
        tags: Vec<String>,

        /// include orphaned cards
        #[arg(long)]
        include_orphans: bool,
    },
}

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
    tags: HashSet<&str>,
    include_orphans: bool,
    leech_method: LeechMethod,
) -> Vec<CardEntry> {
    let today = chrono::Utc::now();
    db.into_values()
        .filter(|c| {
            if let Some(last_reviewed) = c.last_reviewed {
                let next_date = last_reviewed + chrono::Duration::days(c.state.interval as i64);
                today >= next_date
            } else {
                true
            }
        })
        .filter(|c| tags.is_empty() || c.card.tags.iter().any(|t| tags.contains(t.as_str())))
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
            file,
            folder,
            file_types,
            full,
        } => {
            let all_cards = if let Some(folder) = folder {
                parse_cards_from_folder(&folder, &file_types)?
            } else if let Some(file) = file {
                card::parse_file(&file)?
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
            maximum_cards_per_session,
            maximum_duration_of_session,
            leech_failure_threshold,
            leech_method,
            algorithm,
            tags,
            include_orphans,
        } => {
            let db = db::get_db(&args.db)?;
            let state = db::get_global_state(&args.state)?;
            let tags: HashSet<&str> = HashSet::from_iter(tags.iter().map(|s| s.as_str()));
            let mut cards = filter_cards(db, tags, include_orphans, leech_method);
            cards.shuffle(&mut rand::thread_rng());
            let cards: Vec<_> = cards.into_iter().take(maximum_cards_per_session).collect();
            let mut terminal = view::init()?;
            let res = view::revise::App::new(
                cards,
                algorithm,
                state,
                maximum_duration_of_session,
                leech_failure_threshold,
                Box::new(move |cards, state| {
                    let _ = db::update_cards(&args.db, cards);
                    db::write_global_state(&args.state, state)
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
            tags: vec!["#flashcard".to_string()],
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
        let leech_method = LeechMethod::Skip;
        let cards = filter_cards(db, tags, include_orphans, leech_method);
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
        let cards = filter_cards(db, tags, include_orphans, leech_method);
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
        let cards = filter_cards(db, tags, include_orphans, leech_method);
        assert_eq!(cards.len(), 1);
    }

    #[test]
    fn test_filter_cards_interval() {
        let mut db = get_card_db();
        let entry = db.get_mut(&blake3::hash(b"test")).unwrap();
        entry.state.interval = 1;
        entry.last_reviewed = Some(chrono::Utc::now());
        let tags = HashSet::new();
        let include_orphans = false;
        let leech_method = LeechMethod::Skip;
        let cards = filter_cards(db, tags, include_orphans, leech_method);
        assert!(cards.is_empty());
    }

    #[test]
    fn test_filter_cards_matching_tags() {
        let db = get_card_db();
        let tags = HashSet::from_iter(vec!["#flashcard"]);
        let include_orphans = false;
        let leech_method = LeechMethod::Skip;
        let cards = filter_cards(db, tags, include_orphans, leech_method);
        assert_eq!(cards.len(), 1);
    }

    #[test]
    fn test_filter_cards_non_matching_tags() {
        let db = get_card_db();
        let tags = HashSet::from_iter(vec!["#foo"]);
        let include_orphans = false;
        let leech_method = LeechMethod::Skip;
        let cards = filter_cards(db, tags, include_orphans, leech_method);
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
        let cards = filter_cards(db, tags, include_orphans, leech_method);
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
        let cards = filter_cards(db, tags, include_orphans, leech_method);
        assert!(cards.is_empty());
    }
}
