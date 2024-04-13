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
pub struct CardEntry {
    pub card: Card,
    state: CardState,
    pub last_reviewed: Option<DateTime<Utc>>,
    pub failed_count: u64,
    pub orphan: bool,
    pub leech: bool,
}

impl CardEntry {
    fn new(card: Card) -> Self {
        Self {
            card,
            state: CardState::new(),
            last_reviewed: None,
            failed_count: 0,
            orphan: false,
            leech: false,
        }
    }
}

type CardDb = HashMap<blake3::Hash, CardEntry>;

pub fn get_db(db_path: &Path) -> Result<CardDb> {
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

pub fn delete_card(db_path: &Path, id: blake3::Hash) -> Result<()> {
    let mut card_db = get_db(db_path)?;
    if card_db.remove(&id).is_none() {
        bail!("Card with id {} not found", id);
    }
    write_db(db_path, &card_db)
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
    fn existing_ids(card_db: &CardDb) -> HashSet<blake3::Hash> {
        card_db.keys().cloned().collect()
    }

    let mut found_card_db: CardDb = found_cards
        .iter()
        .map(|card| (card.id, CardEntry::new(card.clone())))
        .collect();
    let found_ids: HashSet<_> = found_card_db.keys().cloned().collect();

    let mut new_ctr = 0;
    let mut orphan_ctr = 0;
    let mut unorphan_ctr = 0;
    let mut updated_ctr = 0;

    // update existing cards
    for id in existing_ids(&card_db).intersection(&found_ids) {
        let mut entry = card_db.remove(id).unwrap();
        let new = found_card_db.remove(id).unwrap();
        if entry.card != new.card {
            entry.card = new.card;
            updated_ctr += 1;
        }
        if entry.orphan {
            entry.orphan = false;
            unorphan_ctr += 1;
        }
        card_db.insert(*id, entry);
    }

    // new cards
    for id in found_ids.difference(&existing_ids(&card_db)) {
        card_db.insert(*id, found_card_db.remove(id).unwrap());
        new_ctr += 1;
    }

    // orphaned cards
    for id in existing_ids(&card_db).difference(&found_ids) {
        if let Some(entry) = card_db.get_mut(id) {
            entry.orphan = true;
        }
        orphan_ctr += 1;
    }

    if new_ctr == 0 {
        log::info!("No new cards found");
    } else {
        log::info!("Inserted {} new cards", new_ctr);
    }

    if updated_ctr > 0 {
        log::info!("Updated {} cards", updated_ctr);
    }

    if orphan_ctr > 0 {
        log::warn!("Found {} orphaned cards", orphan_ctr);
    }

    if unorphan_ctr > 0 {
        log::info!("Unorphaned {} cards", unorphan_ctr);
    }

    write_db(db_path, &card_db)
}
