use std::{
    collections::{HashMap, HashSet},
    fs,
    io::Write,
    path::Path,
};

use crate::{
    algorithm::{CardState, OptimalFactorMatrix},
    card::Card,
};
use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Atomically write content to a file using temp file + rename
fn atomic_write(path: &Path, content: &str) -> Result<()> {
    let temp_path = path.with_extension("tmp");

    // Clean up any stale temp file
    let _ = fs::remove_file(&temp_path);

    // Write to temporary file first
    let mut temp_file = fs::File::create(&temp_path)
        .with_context(|| format!("Failed to create temp file: {}", temp_path.display()))?;

    temp_file
        .write_all(content.as_bytes())
        .with_context(|| format!("Failed to write to temp file: {}", temp_path.display()))?;

    temp_file
        .sync_all()
        .with_context(|| format!("Failed to sync temp file: {}", temp_path.display()))?;

    // Atomically replace the original file
    fs::rename(&temp_path, path).with_context(|| {
        format!(
            "Failed to rename {} to {}",
            temp_path.display(),
            path.display()
        )
    })?;

    drop(temp_file);

    Ok(())
}
// Clone for tests
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CardEntry {
    pub added: DateTime<Utc>,
    pub card: Card,
    pub last_revised: Option<DateTime<Utc>>,
    pub leech: bool,
    pub orphan: bool,
    pub revise_count: u64,
    pub state: CardState,
}

impl CardEntry {
    pub fn new(card: Card) -> Self {
        Self {
            added: Utc::now(),
            card,
            last_revised: None,
            leech: false,
            orphan: false,
            revise_count: 0,
            state: CardState::default(),
        }
    }
}

pub type CardDb = HashMap<blake3::Hash, CardEntry>;

#[derive(Debug, Serialize, Default, Deserialize, PartialEq)]
pub struct GlobalState {
    pub optimal_factor_matrix: OptimalFactorMatrix,
    pub last_revise_session: Option<DateTime<Utc>>,
    pub mean_q: Option<f64>,
    pub total_cards_revised: u64,
}

pub fn get_global_state(state_path: &Path) -> Result<GlobalState> {
    if state_path.exists() {
        let data = fs::read_to_string(state_path)
            .with_context(|| format!("Failed to read `{}`", state_path.display()))?;
        match serde_json::from_str(&data) {
            Ok(state) => Ok(state),
            Err(_) => {
                log::warn!("Global state corrupted, creating a new one");
                Ok(GlobalState::default())
            }
        }
    } else {
        log::info!("No global state found, using default");
        Ok(GlobalState::default())
    }
}

pub fn refresh_global_state(state: &mut GlobalState) {
    let now = chrono::Utc::now();
    // Reset mean_q if last revision session was more than a week ago
    if let Some(last_session) = state.last_revise_session {
        if now - last_session > chrono::Duration::weeks(1) {
            log::info!("Resetting mean_q and total_cards_revised");
            state.total_cards_revised = 0;
            state.mean_q = None;
        }
    }
    state.last_revise_session = Some(now);
}

pub fn write_global_state(state_path: &Path, state: &GlobalState) -> Result<()> {
    let json_content = serde_json::to_string(state).context("Failed to serialize global state")?;
    atomic_write(state_path, &json_content)
        .with_context(|| format!("Error writing to `{}`", state_path.display()))
}

pub fn get_db(db_path: &Path) -> Result<CardDb> {
    if !db_path.exists() {
        log::info!("No db found, creating new one");
        return Ok(HashMap::new());
    }
    let data = fs::read_to_string(db_path)
        .with_context(|| format!("Error reading `{}`", db_path.display()))?;

    // Handle empty file case
    if data.trim().is_empty() {
        return Ok(HashMap::new());
    }

    let data: Vec<CardEntry> = serde_json::from_str(&data).context("Failed to deserialise db")?;
    Ok(data
        .into_iter()
        .map(|entry| (entry.card.id, entry))
        .collect())
}

fn write_db(db_path: &Path, db: &CardDb) -> Result<()> {
    let data = db.values().collect::<Vec<_>>();
    let json_content = serde_json::to_string(&data).context("Error serializing db")?;
    atomic_write(db_path, &json_content)
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
        log::info!("No cards to add to db");
        return Ok(());
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
        .into_iter()
        .map(|card| (card.id, CardEntry::new(card)))
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
mod tests {

    use ordered_float::OrderedFloat;
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
            response: vec!["bar".to_string()],
            tags: HashSet::from(["foo".to_string()]),
        };
        let card2 = Card {
            id: blake3::hash(b"baz"),
            file: Path::new("baz").to_path_buf(),
            line: 0,
            prompt: "baz".to_string(),
            response: vec!["bar".to_string()],
            tags: HashSet::from(["baz".to_string()]),
        };
        vec![
            CardEntry {
                added: "2012-12-12T12:12:12Z".parse::<DateTime<Utc>>().unwrap(),
                card,
                last_revised: None,
                leech: false,
                orphan: true,
                revise_count: 1,
                state: CardState::default(),
            },
            CardEntry {
                added: "2011-11-11T11:11:11Z".parse::<DateTime<Utc>>().unwrap(),
                card: card2,
                last_revised: "2012-12-12T12:12:12Z".parse::<DateTime<Utc>>().ok(),
                leech: true,
                orphan: false,
                revise_count: 2,
                state: CardState::default(),
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
        let mut state = GlobalState::default();
        let file = write_a_global_state(&state);
        let read_state = get_global_state(&file.path()).unwrap();
        assert_eq!(state, read_state);

        state.optimal_factor_matrix = OptimalFactorMatrix::new();
        state
            .optimal_factor_matrix
            .insert(1, HashMap::from([(OrderedFloat(2.4), 4.6)]));
        let file = write_a_global_state(&state);
        let read_state = get_global_state(&file.path()).unwrap();
        assert_eq!(state, read_state);
    }

    #[test]
    fn test_refresh_global_state() {
        let mut state = GlobalState::default();
        assert!(state.last_revise_session.is_none());
        refresh_global_state(&mut state);
        assert!(state.last_revise_session.is_some());
        assert_eq!(state.total_cards_revised, 0);
        assert!(state.mean_q.is_none());

        state.last_revise_session = Some(Utc::now() - chrono::Duration::weeks(2));
        state.mean_q = Some(2.4);
        state.total_cards_revised = 4;
        refresh_global_state(&mut state);
        assert!(state.last_revise_session.is_some());
        assert_eq!(state.total_cards_revised, 0);
        assert!(state.mean_q.is_none());
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
            response: vec!["new".to_string()],
            tags: HashSet::from(["new".to_string()]),
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
            response: vec!["new".to_string()],
            tags: HashSet::from(["new".to_string()]),
        };
        update_db(&file.path(), vec![card.clone()], false).unwrap();
        let read_db = get_db(&file.path()).unwrap();
        db.insert(card.id, CardEntry::new(card));
        assert_eq!(db.len(), read_db.len());
        assert_eq!(
            db.keys().collect::<HashSet<_>>(),
            read_db.keys().collect::<HashSet<_>>()
        );
    }

    #[test]
    fn test_empty_db_operations() {
        let file = tempfile::NamedTempFile::new().unwrap();

        // Test operations on empty DB
        let empty_db = get_db(&file.path()).unwrap();
        assert!(empty_db.is_empty());

        // Test updating empty DB
        update_db(&file.path(), vec![], true).unwrap();
        assert!(get_db(&file.path()).unwrap().is_empty());

        // Test deleting from empty DB
        let result = delete_card(&file.path(), blake3::hash(b"nonexistent"));
        assert!(result.is_err());
    }

    #[test]
    fn test_concurrent_card_updates() {
        let (file, _) = write_a_db(get_card_entries());
        let entries = get_card_entries();

        // Modify same card twice with different states
        let mut entry1 = entries[0].clone();
        let mut entry2 = entries[0].clone();

        entry1.state.interval = 5;
        entry2.state.interval = 10;

        // Update with both modifications
        update_cards(&file.path(), vec![entry1.clone(), entry2.clone()]).unwrap();

        // Last update should win
        let read_db = get_db(&file.path()).unwrap();
        assert_eq!(read_db.get(&entry1.card.id).unwrap().state.interval, 10);
    }

    #[test]
    fn test_update_db_with_duplicate_cards() {
        let (file, _) = write_a_db(vec![]);
        let card = Card {
            id: blake3::hash(b"duplicate"),
            file: Path::new("test").to_path_buf(),
            line: 0,
            prompt: "test".to_string(),
            response: vec!["test".to_string()],
            tags: HashSet::new(),
        };

        // Add same card twice in single update
        let cards = vec![card.clone(), card.clone()];
        update_db(&file.path(), cards, true).unwrap();

        let read_db = get_db(&file.path()).unwrap();
        assert_eq!(read_db.len(), 1); // Should only store one copy
    }

    #[test]
    fn test_global_state_edge_cases() {
        let mut state = GlobalState::default();

        // Test with very old last session
        state.last_revise_session = Some(Utc::now() - chrono::Duration::days(365));
        state.mean_q = Some(4.2);
        state.total_cards_revised = 100;

        refresh_global_state(&mut state);
        assert_eq!(state.total_cards_revised, 0);
        assert!(state.mean_q.is_none());

        // Test with future timestamp (should handle gracefully)
        state.last_revise_session = Some(Utc::now() + chrono::Duration::days(1));
        refresh_global_state(&mut state);
        assert!(state.last_revise_session.unwrap() <= Utc::now());
    }

    #[test]
    fn test_atomic_write() {
        use tempfile::NamedTempFile;

        let temp_file = NamedTempFile::new().unwrap();
        let file_path = temp_file.path();

        // Test successful atomic write
        let content = "test content";
        atomic_write(file_path, content).unwrap();

        let read_content = fs::read_to_string(file_path).unwrap();
        assert_eq!(read_content, content);

        // Test that temp file is cleaned up
        let temp_path = file_path.with_extension("tmp");
        assert!(!temp_path.exists());

        // Test overwriting existing file
        let new_content = "new test content";
        atomic_write(file_path, new_content).unwrap();

        let read_content = fs::read_to_string(file_path).unwrap();
        assert_eq!(read_content, new_content);

        // Test with empty content
        atomic_write(file_path, "").unwrap();
        let read_content = fs::read_to_string(file_path).unwrap();
        assert_eq!(read_content, "");
    }

    #[test]
    fn test_corrupted_db_file() {
        use tempfile::NamedTempFile;

        let temp_file = NamedTempFile::new().unwrap();
        let file_path = temp_file.path();

        // Write invalid JSON
        fs::write(file_path, "invalid json content {").unwrap();

        // Should handle corrupted file gracefully
        let result = get_db(file_path);
        assert!(result.is_err());

        // Test with empty file
        fs::write(file_path, "").unwrap();
        let result = get_db(file_path);
        assert!(result.is_ok());
        let db = result.unwrap();
        assert!(db.is_empty());

        // Test with partial JSON
        fs::write(file_path, "[{\"card\":{\"id\":[1,2,3").unwrap();
        let result = get_db(file_path);
        assert!(result.is_err());
    }

    #[test]
    fn test_corrupted_global_state_file() {
        use tempfile::NamedTempFile;

        let temp_file = NamedTempFile::new().unwrap();
        let file_path = temp_file.path();

        // Write invalid JSON
        fs::write(file_path, "not json at all").unwrap();

        // Should handle corrupted state file gracefully by creating default
        let result = get_global_state(file_path);
        assert!(result.is_ok());
        let state = result.unwrap();
        assert_eq!(state, GlobalState::default());

        // Test with partial JSON
        fs::write(file_path, "{\"mean_q\":").unwrap();
        let result = get_global_state(file_path);
        assert!(result.is_ok());
    }

    #[test]
    fn test_extreme_interval_values() {
        use crate::algorithm::{new_algorithm, Algo, Quality};
        use tempfile::NamedTempFile;

        let temp_file = NamedTempFile::new().unwrap();
        let file_path = temp_file.path();

        // Create card with extreme interval
        let card = Card {
            id: blake3::hash(b"test_extreme"),
            file: std::path::PathBuf::from("test.md"),
            line: 1,
            prompt: "test".to_string(),
            response: vec!["test".to_string()],
            tags: std::collections::HashSet::new(),
        };

        let mut entry = CardEntry::new(card);
        entry.state.interval = u64::MAX - 1000;
        entry.state.ease_factor = 10.0; // Very high ease factor

        let mut db = CardDb::new();
        db.insert(entry.card.id, entry);

        // Test that write/read works with extreme values
        write_db(file_path, &db).unwrap();
        let loaded_db = get_db(file_path).unwrap();

        let loaded_entry = loaded_db.get(&blake3::hash(b"test_extreme")).unwrap();
        assert_eq!(loaded_entry.state.interval, u64::MAX - 1000);
        assert_eq!(loaded_entry.state.ease_factor, 10.0);

        // Test algorithm with extreme values doesn't panic
        let algorithm = new_algorithm(Algo::SM2);
        let mut state = loaded_entry.state.clone();
        let mut global_state = GlobalState::default();

        algorithm.update_state(&Quality::Perfect, &mut state, &mut global_state);
        assert!(state.interval > 0);
    }

    #[test]
    fn test_concurrent_database_access() {
        use std::sync::{Arc, Barrier};
        use std::thread;
        use tempfile::NamedTempFile;

        let temp_file = NamedTempFile::new().unwrap();
        let file_path = Arc::new(temp_file.path().to_path_buf());

        // Initialize with empty database
        write_db(&file_path, &CardDb::new()).unwrap();

        let barrier = Arc::new(Barrier::new(4));
        let mut handles = vec![];

        // Spawn multiple threads that try to read/write simultaneously
        for _ in 0..4 {
            let file_path = file_path.clone();
            let barrier = barrier.clone();

            let handle = thread::spawn(move || {
                barrier.wait();

                // Each thread tries to read and then write
                let result1 = get_db(&file_path);
                let result2 = write_db(&file_path, &CardDb::new());

                (result1.is_ok(), result2.is_ok())
            });

            handles.push(handle);
        }

        // Collect results - at least some operations should succeed
        let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        let successful_reads = results.iter().filter(|(read, _)| *read).count();
        let successful_writes = results.iter().filter(|(_, write)| *write).count();

        assert_eq!(successful_reads, 4);
        assert!(successful_writes > 0);
    }
}
