mod algorithm;
mod card;
#[macro_use]
extern crate lazy_static;
use crate::algorithm::CardState;
use crate::card::Card;
use anyhow::Result;
use chrono::{DateTime, Utc};
use clap::{Parser, ValueEnum};
use env_logger::Env;
use std::collections::HashSet;
use std::{collections::HashMap, path::PathBuf};
use walkdir::WalkDir;

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

#[derive(Parser, Debug)]
#[command(version, about, long_about=None)]
struct Args {
    /// Use a single file as input
    #[arg(long)]
    file: Option<PathBuf>,

    /// Walk a directory and use all files as input
    #[arg(long, conflicts_with("file"))]
    folder: Option<PathBuf>,

    /// File types to parse
    #[arg(long, default_values_t = ["md".to_string(), "txt".to_string(), "org".to_string()])]
    file_types: Vec<String>,

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
struct CardEntry {
    card: Card,
    state: CardState,
    last_reviewed: DateTime<Utc>,
    failed_count: u64,
}

fn main() -> Result<()> {
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();

    let args = Args::parse();
    let all_cards = if let Some(folder) = args.folder {
        parse_cards_from_folder(&folder, &args.file_types)?
    } else if let Some(file) = args.file {
        card::parse_file(&file)?
    } else {
        vec![]
    };
    let card_db: HashMap<blake3::Hash, Card> =
        all_cards.into_iter().map(|card| (card.id, card)).collect();
    println!("Card DB: {:?}", card_db);
    Ok(())
}
