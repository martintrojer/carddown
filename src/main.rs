mod card;

#[macro_use]
extern crate lazy_static;

use crate::card::Card;
use clap::Parser;
use env_logger::Env;
use rayon::prelude::*;
use std::{collections::HashMap, path::PathBuf};
use walkdir::WalkDir;

#[derive(Parser, Debug)]
#[command(version, about, long_about=None)]
struct Args {
    /// Use a single file as input
    #[arg(long)]
    file: Option<PathBuf>,

    /// Walk a directory and use all files as input
    #[arg(long, conflicts_with("file"))]
    folder: Option<PathBuf>,
}

// walk file tree and parse all files
fn walk_files(folder: &PathBuf) -> Vec<Card> {
    let all_cards = WalkDir::new(folder)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .filter(|e| e.path().extension().unwrap() == "md")
        .map(|e| PathBuf::from(e.path()))
        .collect::<Vec<PathBuf>>()
        .par_iter()
        .map(|file| match card::parse_file(file) {
            Ok(cards) => cards,
            Err(e) => {
                log::error!("Error parsing file: {:?} {}", file, e);
                vec![]
            }
        })
        .collect::<Vec<Vec<Card>>>();
    all_cards.into_iter().flatten().collect()
}

fn main() {
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();

    let args = Args::parse();
    let all_cards = if let Some(folder) = args.folder {
        walk_files(&folder)
    } else if let Some(file) = args.file {
        card::parse_file(&file).unwrap()
    } else {
        vec![]
    };
    let card_db: HashMap<blake3::Hash, Card> =
        all_cards.into_iter().map(|card| (card.id, card)).collect();
    println!("Card DB: {:?}", card_db);
}
