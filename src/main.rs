mod algorithm;
mod card;
mod db;
mod tui;

use crate::card::Card;
use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};
use env_logger::Env;
use std::collections::HashSet;
use std::path::PathBuf;
use tui::App;
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
}

#[derive(Debug, Clone, ValueEnum)]
enum LeechMethod {
    Normal,
    Skip,
    Warn,
}

#[derive(Debug, Clone, ValueEnum)]
enum Algo {
    SM2,
    SM5,
    Simple8,
    Leitner,
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
        } => {
            let all_cards = if let Some(folder) = folder {
                parse_cards_from_folder(&folder, &file_types)?
            } else if let Some(file) = file {
                card::parse_file(&file)?
            } else {
                vec![]
            };
            db::update_db(&args.db, all_cards)?;
        }
        Commands::Audit {} => {
            let mut terminal = tui::init()?;
            let db = db::get_db(&args.db)?;
            let cards = db.into_values().filter(|c| c.orphan || c.leech).collect();
            let app_result = App::new(cards, Box::new(move |id| db::delete_card(&args.db, id)))
                .run(&mut terminal);
            tui::restore()?;
            app_result?
        }
        _ => {}
    }

    Ok(())
}
