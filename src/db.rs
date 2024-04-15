use std::{
    collections::{HashMap, HashSet},
    fs,
    path::Path,
};

use crate::{
    algorithm::{CardState, Quality},
    card::Card,
};
use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct CardEntry {
    pub card: Card,
    pub state: CardState,
    pub last_reviewed: Option<DateTime<Utc>>,
    pub failed_count: u64,
    pub orphan: bool,
    pub leech: bool,
}

impl CardEntry {
    pub fn new(card: Card) -> Self {
        Self {
            card,
            state: CardState::default(),
            last_reviewed: None,
            failed_count: 0,
            orphan: false,
            leech: false,
        }
    }
}

pub type CardDb = HashMap<blake3::Hash, CardEntry>;

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct GlobalState {
    optimal_factor: HashMap<Quality, f64>,
}

impl Default for GlobalState {
    fn default() -> Self {
        let mut optimal_factor = HashMap::new();
        optimal_factor.insert(Quality::Perfect, 2.5);
        optimal_factor.insert(Quality::CorrectWithHesitation, 2.5);
        optimal_factor.insert(Quality::CorrectWithDifficulty, 2.5);
        optimal_factor.insert(Quality::IncorrectButEasyToRecall, 2.5);
        optimal_factor.insert(Quality::IncorrectButRemembered, 2.5);
        optimal_factor.insert(Quality::IncorrectAndForgotten, 2.5);
        Self { optimal_factor }
    }
}

pub fn get_global_state(state_path: &Path) -> Result<GlobalState> {
    if state_path.exists() {
        let data = fs::read_to_string(state_path)
            .with_context(|| format!("Failed to read `{}`", state_path.display()))?;
        serde_json::from_str(&data).context("Failed to deserialize state.json")
    } else {
        log::info!("No global state found, using default");
        Ok(GlobalState::default())
    }
}

pub fn write_global_state(state_path: &Path, state: &GlobalState) -> Result<()> {
    fs::write(
        state_path,
        serde_json::to_string(state).context("Failed to serialize state.json")?,
    )
    .with_context(|| format!("Error writing to `{}`", state_path.display()))
}

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

pub fn update_cards(db_path: &Path, cards: Vec<CardEntry>) -> Result<()> {
    let mut card_db = get_db(db_path)?;
    for card in cards {
        card_db.insert(card.card.id, card);
    }
    write_db(db_path, &card_db)
}

pub fn update_db(db_path: &Path, found_cards: Vec<Card>, full: bool) -> Result<()> {
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
    if full {
        for id in existing_ids(&card_db).difference(&found_ids) {
            if let Some(entry) = card_db.get_mut(id) {
                entry.orphan = true;
            }
            orphan_ctr += 1;
        }
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

#[cfg(test)]
mod test {

    use tempfile::NamedTempFile;

    use super::*;

    fn write_a_db(data: Vec<CardEntry>) -> (NamedTempFile, CardDb) {
        let file = tempfile::NamedTempFile::new().unwrap();
        let db = data
            .into_iter()
            .map(|entry| (entry.card.id, entry))
            .collect();
        write_db(&file.path(), &db).unwrap();
        (file, db)
    }

    fn write_a_global_state(state: &GlobalState) -> NamedTempFile {
        let file = tempfile::NamedTempFile::new().unwrap();
        write_global_state(&file.path(), state).unwrap();
        file
    }

    fn get_card_entries() -> Vec<CardEntry> {
        let card = Card {
            id: blake3::hash(b"foo"),
            file: Path::new("foo").to_path_buf(),
            line: 0,
            prompt: "foo".to_string(),
            response: "bar".to_string(),
            tags: vec!["#foo".to_string()],
        };
        let card2 = Card {
            id: blake3::hash(b"baz"),
            file: Path::new("baz").to_path_buf(),
            line: 0,
            prompt: "baz".to_string(),
            response: "bar".to_string(),
            tags: vec!["#baz".to_string()],
        };
        vec![
            CardEntry {
                card,
                state: CardState::default(),
                last_reviewed: None,
                failed_count: 0,
                orphan: true,
                leech: false,
            },
            CardEntry {
                card: card2,
                state: CardState::default(),
                last_reviewed: "2012-12-12T12:12:12Z".parse::<DateTime<Utc>>().ok(),
                failed_count: 1,
                orphan: false,
                leech: true,
            },
        ]
    }

    #[test]
    fn test_get_db() {
        let (file, db) = write_a_db(get_card_entries());
        let read_db = get_db(&file.path()).unwrap();
        assert_eq!(db, read_db);
    }

    #[test]
    fn test_get_global_state() {
        let state = GlobalState::default();
        let file = write_a_global_state(&state);
        let read_state = get_global_state(&file.path()).unwrap();
        assert_eq!(state, read_state);
    }

    #[test]
    fn test_delete_card() {
        let (file, mut db) = write_a_db(get_card_entries());
        let id = db.keys().next().unwrap().clone();
        delete_card(&file.path(), id).unwrap();
        let read_db = get_db(&file.path()).unwrap();
        db.remove(&id);
        assert_eq!(db, read_db);
    }

    #[test]
    fn test_update_cards() {
        let (file, mut db) = write_a_db(get_card_entries());
        let mut entry = get_card_entries().pop().unwrap();
        db.get_mut(&entry.card.id).unwrap().state.interval = 1;
        entry.state.interval = 1;
        update_cards(&file.path(), vec![entry]).unwrap();
        let read_db = get_db(&file.path()).unwrap();
        assert_eq!(db, read_db);
    }

    #[test]
    fn test_update_db_update_card() {
        let (file, _) = write_a_db(get_card_entries());
        let mut entry = get_card_entries().pop().unwrap();
        entry.card.prompt = "new prompt".to_string();
        update_db(&file.path(), vec![entry.card.clone()], false).unwrap();
        let read_db = get_db(&file.path()).unwrap();
        assert_eq!(
            read_db.get(&entry.card.id).unwrap().card.prompt,
            "new prompt"
        );
    }

    #[test]
    fn test_update_db_unorphan() {
        let (file, _) = write_a_db(get_card_entries());
        let entry = get_card_entries().remove(0);
        assert!(entry.orphan);
        update_db(&file.path(), vec![entry.card.clone()], false).unwrap();
        let read_db = get_db(&file.path()).unwrap();
        assert_eq!(read_db.get(&entry.card.id).unwrap().orphan, false);
    }

    #[test]
    fn test_update_db_orphan() {
        let (file, _) = write_a_db(get_card_entries());
        let entry = get_card_entries().remove(1);
        assert!(!entry.orphan);
        let card = Card {
            id: blake3::hash(b"new"),
            file: Path::new("new").to_path_buf(),
            line: 0,
            prompt: "new".to_string(),
            response: "new".to_string(),
            tags: vec!["#new".to_string()],
        };
        update_db(&file.path(), vec![card], true).unwrap();
        let read_db = get_db(&file.path()).unwrap();
        assert!(read_db.get(&entry.card.id).unwrap().orphan);
    }

    #[test]
    fn test_update_db_new_card() {
        let (file, mut db) = write_a_db(get_card_entries());
        let card = Card {
            id: blake3::hash(b"new"),
            file: Path::new("new").to_path_buf(),
            line: 0,
            prompt: "new".to_string(),
            response: "new".to_string(),
            tags: vec!["#new".to_string()],
        };
        update_db(&file.path(), vec![card.clone()], false).unwrap();
        let read_db = get_db(&file.path()).unwrap();
        db.insert(
            card.id,
            CardEntry {
                card,
                state: CardState::default(),
                last_reviewed: None,
                failed_count: 0,
                orphan: false,
                leech: false,
            },
        );
        assert_eq!(db, read_db);
    }
}
