use std::{
    collections::{HashMap, HashSet},
    fs,
    path::Path,
};

use crate::{algorithm::CardState, card::Card};
use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
struct CardEntry {
    card: Card,
    state: CardState,
    last_reviewed: Option<DateTime<Utc>>,
    failed_count: u64,
    orphan: bool,
}

impl CardEntry {
    fn new(card: Card) -> Self {
        Self {
            card,
            state: CardState::new(),
            last_reviewed: None,
            failed_count: 0,
            orphan: false,
        }
    }
}

type CardDb = HashMap<blake3::Hash, CardEntry>;

fn get_db(db_path: &Path) -> Result<CardDb> {
    let data: Vec<CardEntry> = serde_json::from_str(
        &fs::read_to_string(db_path)
            .with_context(|| format!("Error reading `{}`", db_path.display()))?,
    )
    .context("Error deserializing db")?;
    Ok(data
        .into_iter()
        .map(|entry| (entry.card.id, entry))
        .collect())
}

fn write_db(db_path: &Path, db: &CardDb) -> Result<()> {
    let data = db.values().collect::<Vec<_>>();
    fs::write(
        db_path,
        serde_json::to_string(&data).context("Error serializing db")?,
    )
    .with_context(|| format!("Error writing to `{}`", db_path.display()))
}

pub fn update_db(db_path: &Path, found_cards: Vec<Card>) -> Result<()> {
    if found_cards.is_empty() {
        bail!("No cards to add to db");
    }
    let mut card_db: CardDb = if !db_path.exists() {
        HashMap::new()
    } else {
        get_db(db_path)?
    };
    let mut found_card_db: CardDb = found_cards
        .iter()
        .map(|card| (card.id, CardEntry::new(card.clone())))
        .collect();
    let existing_ids: HashSet<_> = card_db.keys().cloned().collect();
    let found_ids: HashSet<_> = found_card_db.keys().cloned().collect();
    let mut new_ctr = 0;
    let mut orphan_ctr = 0;

    // new cards
    for id in found_ids.difference(&existing_ids) {
        card_db.insert(*id, found_card_db.remove(id).unwrap());
        new_ctr += 1;
    }

    // orphaned cards
    for id in existing_ids.difference(&found_ids) {
        if let Some(entry) = card_db.get_mut(id) {
            entry.orphan = true;
        }
        orphan_ctr += 1;
    }

    if new_ctr == 0 {
        log::info!("No new cards found");
    } else {
        log::info!(
            "Inserted {} new cards",
            found_ids.difference(&existing_ids).count()
        );
    }

    if orphan_ctr > 0 {
        log::warn!(
            "Found {} orphaned cards",
            existing_ids.difference(&found_ids).count()
        );
    }

    write_db(db_path, &card_db)
}
