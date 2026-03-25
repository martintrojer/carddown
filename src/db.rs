use std::{
    collections::{HashMap, HashSet},
    path::Path,
};

use crate::{
    algorithm::{CardState, OptimalFactorMatrix},
    card::Card,
};
use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

const SCHEMA_VERSION: u32 = 1;

fn open_db(db_path: &Path) -> Result<Connection> {
    let conn = Connection::open(db_path)
        .with_context(|| format!("Failed to open database: {}", db_path.display()))?;
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
    Ok(conn)
}

fn ensure_schema(conn: &Connection) -> Result<()> {
    let version: u32 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version == SCHEMA_VERSION {
        return Ok(());
    }
    if version != 0 {
        // Drop and recreate — the db is a derived cache, not the source of truth.
        conn.execute_batch(
            "DROP TABLE IF EXISTS cards;
             DROP TABLE IF EXISTS global_state;
             DROP TABLE IF EXISTS scan_index;",
        )?;
    }
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS cards (
            id BLOB NOT NULL PRIMARY KEY,
            file TEXT NOT NULL,
            line INTEGER NOT NULL,
            prompt TEXT NOT NULL,
            response TEXT NOT NULL,
            tags TEXT NOT NULL,
            added TEXT NOT NULL,
            last_revised TEXT,
            revise_count INTEGER NOT NULL DEFAULT 0,
            leech INTEGER NOT NULL DEFAULT 0,
            orphan INTEGER NOT NULL DEFAULT 0,
            ease_factor REAL NOT NULL DEFAULT 2.5,
            interval INTEGER NOT NULL DEFAULT 0,
            repetitions INTEGER NOT NULL DEFAULT 0,
            failed_count INTEGER NOT NULL DEFAULT 0
        );
        CREATE INDEX IF NOT EXISTS idx_cards_status ON cards (orphan, leech);
        CREATE TABLE IF NOT EXISTS global_state (
            id INTEGER PRIMARY KEY CHECK (id = 1),
            optimal_factor_matrix TEXT NOT NULL DEFAULT '{}',
            last_revise_session TEXT,
            mean_q REAL,
            total_cards_revised INTEGER NOT NULL DEFAULT 0
        );
        CREATE TABLE IF NOT EXISTS scan_index (
            file_path TEXT PRIMARY KEY,
            mtime INTEGER NOT NULL
        );
        INSERT OR IGNORE INTO global_state (id) VALUES (1);",
    )?;
    conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
    Ok(())
}

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

pub type ScanIndex = HashMap<String, u64>;

// --- Card operations ---

fn row_to_card_entry(row: &rusqlite::Row) -> rusqlite::Result<CardEntry> {
    let id_bytes: Vec<u8> = row.get(0)?;
    let file: String = row.get(1)?;
    let line: i64 = row.get(2)?;
    let prompt: String = row.get(3)?;
    let response_json: String = row.get(4)?;
    let tags_json: String = row.get(5)?;
    let added_str: String = row.get(6)?;
    let last_revised_str: Option<String> = row.get(7)?;
    let revise_count: i64 = row.get(8)?;
    let leech: bool = row.get(9)?;
    let orphan: bool = row.get(10)?;
    let ease_factor: f64 = row.get(11)?;
    let interval: i64 = row.get(12)?;
    let repetitions: i64 = row.get(13)?;
    let failed_count: i64 = row.get(14)?;

    let hash_bytes: [u8; 32] = id_bytes.try_into().map_err(|_| {
        rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Blob,
            "invalid hash length".into(),
        )
    })?;
    let id = blake3::Hash::from_bytes(hash_bytes);
    let response: Vec<String> = serde_json::from_str(&response_json).unwrap_or_default();
    let tags: HashSet<String> = serde_json::from_str(&tags_json).unwrap_or_default();
    let added = added_str
        .parse::<DateTime<Utc>>()
        .unwrap_or_else(|_| Utc::now());
    let last_revised = last_revised_str.and_then(|s| s.parse::<DateTime<Utc>>().ok());

    Ok(CardEntry {
        added,
        card: Card {
            id,
            file: file.into(),
            line: line as u64,
            prompt,
            response,
            tags,
        },
        last_revised,
        leech,
        orphan,
        revise_count: revise_count as u64,
        state: CardState {
            ease_factor,
            interval: interval as u64,
            repetitions: repetitions as u64,
            failed_count: failed_count as u64,
        },
    })
}

pub fn get_db(db_path: &Path) -> Result<CardDb> {
    if !db_path.exists() {
        log::debug!("No db found, creating new one");
        return Ok(HashMap::new());
    }
    let conn = open_db(db_path)?;
    ensure_schema(&conn)?;

    let mut stmt = conn.prepare(
        "SELECT id, file, line, prompt, response, tags, added, last_revised,
                revise_count, leech, orphan, ease_factor, interval, repetitions, failed_count
         FROM cards",
    )?;
    let entries = stmt
        .query_map([], row_to_card_entry)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(entries.into_iter().map(|e| (e.card.id, e)).collect())
}

pub fn write_db(db_path: &Path, db: &CardDb) -> Result<()> {
    let conn = open_db(db_path)?;
    ensure_schema(&conn)?;

    let tx = conn.unchecked_transaction()?;
    tx.execute("DELETE FROM cards", [])?;
    {
        let mut stmt = tx.prepare(
            "INSERT INTO cards (id, file, line, prompt, response, tags, added, last_revised,
                                revise_count, leech, orphan, ease_factor, interval, repetitions, failed_count)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
        )?;
        for entry in db.values() {
            insert_card_entry(&mut stmt, entry)?;
        }
    }
    tx.commit()?;
    Ok(())
}

fn insert_card_entry(stmt: &mut rusqlite::Statement, entry: &CardEntry) -> Result<()> {
    stmt.execute(params![
        entry.card.id.as_bytes().as_slice(),
        entry.card.file.to_string_lossy(),
        entry.card.line as i64,
        entry.card.prompt,
        serde_json::to_string(&entry.card.response)?,
        serde_json::to_string(&entry.card.tags)?,
        entry.added.to_rfc3339(),
        entry.last_revised.map(|d| d.to_rfc3339()),
        entry.revise_count as i64,
        entry.leech,
        entry.orphan,
        entry.state.ease_factor,
        entry.state.interval as i64,
        entry.state.repetitions as i64,
        entry.state.failed_count as i64,
    ])?;
    Ok(())
}

pub fn delete_card(db_path: &Path, id: blake3::Hash) -> Result<()> {
    let conn = open_db(db_path)?;
    ensure_schema(&conn)?;
    let deleted = conn.execute(
        "DELETE FROM cards WHERE id = ?1",
        [id.as_bytes().as_slice()],
    )?;
    if deleted == 0 {
        bail!("Card with id {} not found", id);
    }
    Ok(())
}

pub fn update_cards(db_path: &Path, cards: Vec<CardEntry>) -> Result<()> {
    let conn = open_db(db_path)?;
    ensure_schema(&conn)?;

    let tx = conn.unchecked_transaction()?;
    {
        let mut stmt = tx.prepare(
            "INSERT OR REPLACE INTO cards (id, file, line, prompt, response, tags, added, last_revised,
                                           revise_count, leech, orphan, ease_factor, interval, repetitions, failed_count)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
        )?;
        for entry in &cards {
            insert_card_entry(&mut stmt, entry)?;
        }
    }
    tx.commit()?;
    Ok(())
}

// --- Scan stats ---

pub struct ScanStats {
    pub found: usize,
    pub new: usize,
    pub updated: usize,
    pub orphaned: usize,
    pub unorphaned: usize,
}

pub fn update_db(
    db_path: &Path,
    found_cards: Vec<Card>,
    full: bool,
    dry_run: bool,
) -> Result<ScanStats> {
    if found_cards.is_empty() {
        log::debug!("No cards to add to db");
        return Ok(ScanStats {
            found: 0,
            new: 0,
            updated: 0,
            orphaned: 0,
            unorphaned: 0,
        });
    }

    let mut card_db: CardDb = if !db_path.exists() {
        HashMap::new()
    } else {
        get_db(db_path)?
    };

    let mut found_card_db: CardDb = found_cards
        .into_iter()
        .map(|card| (card.id, CardEntry::new(card)))
        .collect();
    let found_ids: HashSet<_> = found_card_db.keys().cloned().collect();

    let mut new_ctr = 0;
    let mut orphan_ctr = 0;
    let mut unorphan_ctr = 0;
    let mut updated_ctr = 0;

    let common_ids: Vec<_> = found_ids
        .iter()
        .filter(|id| card_db.contains_key(id))
        .cloned()
        .collect();
    for id in &common_ids {
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

    let new_ids: Vec<_> = found_ids
        .iter()
        .filter(|id| !card_db.contains_key(id))
        .cloned()
        .collect();
    for id in &new_ids {
        card_db.insert(*id, found_card_db.remove(id).unwrap());
        new_ctr += 1;
    }

    if full {
        let orphan_ids: Vec<_> = card_db
            .keys()
            .filter(|id| !found_ids.contains(id))
            .cloned()
            .collect();
        for id in &orphan_ids {
            if let Some(entry) = card_db.get_mut(id) {
                entry.orphan = true;
            }
            orphan_ctr += 1;
        }
    }

    if new_ctr == 0 {
        log::debug!("No new cards found");
    } else {
        log::debug!("Inserted {new_ctr} new cards");
    }
    if updated_ctr > 0 {
        log::debug!("Updated {updated_ctr} cards");
    }
    if orphan_ctr > 0 {
        log::debug!("Found {orphan_ctr} orphaned cards");
    }
    if unorphan_ctr > 0 {
        log::debug!("Unorphaned {unorphan_ctr} cards");
    }

    let found = found_ids.len();
    if !dry_run {
        write_db(db_path, &card_db)?;
    }
    Ok(ScanStats {
        found,
        new: new_ctr,
        updated: updated_ctr,
        orphaned: orphan_ctr,
        unorphaned: unorphan_ctr,
    })
}

// --- Global state ---

pub fn get_global_state(db_path: &Path) -> Result<GlobalState> {
    if !db_path.exists() {
        log::debug!("No global state found, using default");
        return Ok(GlobalState::default());
    }
    let conn = open_db(db_path)?;
    ensure_schema(&conn)?;

    let mut stmt = conn.prepare(
        "SELECT optimal_factor_matrix, last_revise_session, mean_q, total_cards_revised
         FROM global_state WHERE id = 1",
    )?;
    let result = stmt.query_row([], |row| {
        let ofm_json: String = row.get(0)?;
        let last_session_str: Option<String> = row.get(1)?;
        let mean_q: Option<f64> = row.get(2)?;
        let total: i64 = row.get(3)?;
        Ok((ofm_json, last_session_str, mean_q, total as u64))
    });

    match result {
        Ok((ofm_json, last_session_str, mean_q, total)) => {
            let optimal_factor_matrix: OptimalFactorMatrix =
                serde_json::from_str(&ofm_json).unwrap_or_default();
            let last_revise_session =
                last_session_str.and_then(|s| s.parse::<DateTime<Utc>>().ok());
            Ok(GlobalState {
                optimal_factor_matrix,
                last_revise_session,
                mean_q,
                total_cards_revised: total,
            })
        }
        Err(_) => Ok(GlobalState::default()),
    }
}

pub fn refresh_global_state(state: &mut GlobalState) {
    let now = chrono::Utc::now();
    if let Some(last_session) = state.last_revise_session {
        if now - last_session > chrono::Duration::weeks(1) {
            log::debug!("Resetting mean_q and total_cards_revised");
            state.total_cards_revised = 0;
            state.mean_q = None;
        }
    }
    state.last_revise_session = Some(now);
}

pub fn write_global_state(db_path: &Path, state: &GlobalState) -> Result<()> {
    let conn = open_db(db_path)?;
    ensure_schema(&conn)?;

    let ofm_json = serde_json::to_string(&state.optimal_factor_matrix)?;
    conn.execute(
        "UPDATE global_state SET optimal_factor_matrix = ?1, last_revise_session = ?2,
         mean_q = ?3, total_cards_revised = ?4 WHERE id = 1",
        params![
            ofm_json,
            state.last_revise_session.map(|d| d.to_rfc3339()),
            state.mean_q,
            state.total_cards_revised as i64,
        ],
    )?;
    Ok(())
}

// --- Scan index ---

pub fn load_scan_index(db_path: &Path) -> ScanIndex {
    if !db_path.exists() {
        return HashMap::new();
    }
    let conn = match open_db(db_path) {
        Ok(c) => c,
        Err(_) => return HashMap::new(),
    };
    if ensure_schema(&conn).is_err() {
        return HashMap::new();
    }

    let mut stmt = match conn.prepare("SELECT file_path, mtime FROM scan_index") {
        Ok(s) => s,
        Err(_) => return HashMap::new(),
    };
    let rows = match stmt.query_map([], |row| {
        let path: String = row.get(0)?;
        let mtime: i64 = row.get(1)?;
        Ok((path, mtime as u64))
    }) {
        Ok(r) => r,
        Err(_) => return HashMap::new(),
    };
    rows.filter_map(|r| r.ok()).collect()
}

pub fn save_scan_index(db_path: &Path, index: &ScanIndex) {
    let conn = match open_db(db_path) {
        Ok(c) => c,
        Err(e) => {
            log::warn!("Failed to open db for scan index: {e}");
            return;
        }
    };
    if ensure_schema(&conn).is_err() {
        return;
    }

    let tx = match conn.unchecked_transaction() {
        Ok(t) => t,
        Err(e) => {
            log::warn!("Failed to start transaction for scan index: {e}");
            return;
        }
    };
    if tx.execute("DELETE FROM scan_index", []).is_err() {
        return;
    }
    for (path, mtime) in index {
        if tx
            .execute(
                "INSERT INTO scan_index (file_path, mtime) VALUES (?1, ?2)",
                params![path, *mtime as i64],
            )
            .is_err()
        {
            return;
        }
    }
    if let Err(e) = tx.commit() {
        log::warn!("Failed to save scan index: {e}");
    }
}

// --- JSON import/export for migration ---

/// Auto-migrate from JSON files to SQLite if old-format files exist.
///
/// Detects `cards.json` (and optionally `state.json`, `scan_index.json`)
/// as siblings of `db_path` in `.carddown/`. Migrates data into the new
/// SQLite database and prints a summary. The old JSON files are left in
/// place — the user can delete them manually.
pub fn maybe_migrate_json(db_path: &Path) -> Result<()> {
    if db_path.exists() {
        return Ok(());
    }
    let dir = match db_path.parent() {
        Some(d) => d,
        None => return Ok(()),
    };
    let cards_json = dir.join("cards.json");
    if !cards_json.exists() {
        return Ok(());
    }

    eprintln!("Migrating from JSON to SQLite...");

    let card_db = load_json_cards(&cards_json)?;
    let card_count = card_db.len();

    // Write cards to SQLite
    if !card_db.is_empty() {
        write_db(db_path, &card_db)?;
    }

    // Migrate state.json
    let state_json = dir.join("state.json");
    if state_json.exists() {
        if let Ok(data) = std::fs::read_to_string(&state_json) {
            if let Ok(state) = serde_json::from_str::<GlobalState>(&data) {
                write_global_state(db_path, &state)?;
            }
        }
    }

    // Migrate scan_index.json
    let index_json = dir.join("scan_index.json");
    if index_json.exists() {
        if let Ok(data) = std::fs::read_to_string(&index_json) {
            if let Ok(index) = serde_json::from_str::<ScanIndex>(&data) {
                save_scan_index(db_path, &index);
            }
        }
    }

    eprintln!(
        "Migrated {card_count} card(s) to {}. Old JSON files kept in place.",
        db_path.display()
    );
    Ok(())
}

/// Load a CardDb from an old-format JSON file (pre-0.3.0 cards.json).
pub fn load_json_cards(json_path: &Path) -> Result<CardDb> {
    let data = std::fs::read_to_string(json_path)
        .with_context(|| format!("Failed to read {}", json_path.display()))?;
    if data.trim().is_empty() {
        return Ok(HashMap::new());
    }
    let entries: Vec<CardEntry> =
        serde_json::from_str(&data).context("Failed to deserialise JSON cards")?;
    Ok(entries.into_iter().map(|e| (e.card.id, e)).collect())
}

/// Export cards to JSON format (for migration or backup).
pub fn export_json_cards(db: &CardDb) -> Result<String> {
    let data: Vec<&CardEntry> = db.values().collect();
    serde_json::to_string_pretty(&data).context("Failed to serialise cards to JSON")
}

/// Export global state to JSON format (for migration or backup).
pub fn export_json_state(state: &GlobalState) -> Result<String> {
    serde_json::to_string_pretty(state).context("Failed to serialise state to JSON")
}

#[cfg(test)]
mod tests {
    use super::*;
    use ordered_float::OrderedFloat;
    use tempfile::NamedTempFile;

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

    fn write_a_db(data: Vec<CardEntry>) -> (NamedTempFile, CardDb) {
        let file = NamedTempFile::new().unwrap();
        let db: CardDb = data.into_iter().map(|e| (e.card.id, e)).collect();
        write_db(file.path(), &db).unwrap();
        (file, db)
    }

    #[test]
    fn test_get_db() {
        let (file, db) = write_a_db(get_card_entries());
        let read_db = get_db(file.path()).unwrap();
        assert_eq!(db.len(), read_db.len());
        for (id, entry) in &db {
            let read_entry = read_db.get(id).unwrap();
            assert_eq!(entry.card.prompt, read_entry.card.prompt);
            assert_eq!(entry.revise_count, read_entry.revise_count);
            assert_eq!(entry.leech, read_entry.leech);
            assert_eq!(entry.orphan, read_entry.orphan);
        }
    }

    #[test]
    fn test_get_global_state() {
        let file = NamedTempFile::new().unwrap();
        let mut state = GlobalState::default();
        // Write default state
        {
            let conn = open_db(file.path()).unwrap();
            ensure_schema(&conn).unwrap();
        }
        let read_state = get_global_state(file.path()).unwrap();
        assert_eq!(state, read_state);

        state.optimal_factor_matrix = OptimalFactorMatrix::new();
        state
            .optimal_factor_matrix
            .insert(1, HashMap::from([(OrderedFloat(2.4), 4.6)]));
        write_global_state(file.path(), &state).unwrap();
        let read_state = get_global_state(file.path()).unwrap();
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
        let id = *db.keys().next().unwrap();
        delete_card(file.path(), id).unwrap();
        let read_db = get_db(file.path()).unwrap();
        db.remove(&id);
        assert_eq!(db.len(), read_db.len());
    }

    #[test]
    fn test_update_cards() {
        let (file, _) = write_a_db(get_card_entries());
        let mut entry = get_card_entries().pop().unwrap();
        entry.state.interval = 1;
        update_cards(file.path(), vec![entry.clone()]).unwrap();
        let read_db = get_db(file.path()).unwrap();
        assert_eq!(read_db.get(&entry.card.id).unwrap().state.interval, 1);
    }

    #[test]
    fn test_update_db_update_card() {
        let (file, _) = write_a_db(get_card_entries());
        let mut entry = get_card_entries().pop().unwrap();
        entry.card.prompt = "new prompt".to_string();
        update_db(file.path(), vec![entry.card.clone()], false, false).unwrap();
        let read_db = get_db(file.path()).unwrap();
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
        update_db(file.path(), vec![entry.card.clone()], false, false).unwrap();
        let read_db = get_db(file.path()).unwrap();
        assert!(!read_db.get(&entry.card.id).unwrap().orphan);
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
        update_db(file.path(), vec![card], true, false).unwrap();
        let read_db = get_db(file.path()).unwrap();
        assert!(read_db.get(&entry.card.id).unwrap().orphan);
    }

    #[test]
    fn test_update_db_new_card() {
        let (file, db) = write_a_db(get_card_entries());
        let card = Card {
            id: blake3::hash(b"new"),
            file: Path::new("new").to_path_buf(),
            line: 0,
            prompt: "new".to_string(),
            response: vec!["new".to_string()],
            tags: HashSet::from(["new".to_string()]),
        };
        update_db(file.path(), vec![card], false, false).unwrap();
        let read_db = get_db(file.path()).unwrap();
        assert_eq!(db.len() + 1, read_db.len());
    }

    #[test]
    fn test_empty_db_operations() {
        let file = NamedTempFile::new().unwrap();
        let empty_db = get_db(file.path()).unwrap();
        assert!(empty_db.is_empty());

        update_db(file.path(), vec![], true, false).unwrap();

        let result = delete_card(file.path(), blake3::hash(b"nonexistent"));
        assert!(result.is_err());
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
        let cards = vec![card.clone(), card.clone()];
        update_db(file.path(), cards, true, false).unwrap();
        let read_db = get_db(file.path()).unwrap();
        assert_eq!(read_db.len(), 1);
    }

    #[test]
    fn test_global_state_edge_cases() {
        let mut state = GlobalState {
            last_revise_session: Some(Utc::now() - chrono::Duration::days(365)),
            mean_q: Some(4.2),
            total_cards_revised: 100,
            ..Default::default()
        };
        refresh_global_state(&mut state);
        assert_eq!(state.total_cards_revised, 0);
        assert!(state.mean_q.is_none());

        state.last_revise_session = Some(Utc::now() + chrono::Duration::days(1));
        refresh_global_state(&mut state);
        assert!(state.last_revise_session.unwrap() <= Utc::now());
    }

    #[test]
    fn test_extreme_interval_values() {
        use crate::algorithm::{new_algorithm, Algo, Quality};

        let file = NamedTempFile::new().unwrap();
        let card = Card {
            id: blake3::hash(b"test_extreme"),
            file: std::path::PathBuf::from("test.md"),
            line: 1,
            prompt: "test".to_string(),
            response: vec!["test".to_string()],
            tags: HashSet::new(),
        };
        let mut entry = CardEntry::new(card);
        entry.state.interval = u64::MAX - 1000;
        entry.state.ease_factor = 10.0;

        let mut db = CardDb::new();
        db.insert(entry.card.id, entry);
        write_db(file.path(), &db).unwrap();
        let loaded_db = get_db(file.path()).unwrap();

        let loaded_entry = loaded_db.get(&blake3::hash(b"test_extreme")).unwrap();
        assert_eq!(loaded_entry.state.interval, u64::MAX - 1000);
        assert_eq!(loaded_entry.state.ease_factor, 10.0);

        let algorithm = new_algorithm(Algo::SM2);
        let mut state = loaded_entry.state.clone();
        let mut global_state = GlobalState::default();
        algorithm.update_state(&Quality::Perfect, &mut state, &mut global_state);
        assert!(state.interval > 0);
    }

    #[test]
    fn test_scan_index_roundtrip() {
        let file = NamedTempFile::new().unwrap();
        {
            let conn = open_db(file.path()).unwrap();
            ensure_schema(&conn).unwrap();
        }

        let mut index = ScanIndex::new();
        index.insert("foo.md".to_string(), 12345);
        index.insert("bar.md".to_string(), 67890);
        save_scan_index(file.path(), &index);

        let loaded = load_scan_index(file.path());
        assert_eq!(loaded, index);
    }

    #[test]
    fn test_json_import_export() {
        let (file, db) = write_a_db(get_card_entries());

        // Export to JSON
        let json = export_json_cards(&db).unwrap();

        // Write JSON to a temp file and load it back
        let json_file = NamedTempFile::new().unwrap();
        std::fs::write(json_file.path(), &json).unwrap();
        let loaded_db = load_json_cards(json_file.path()).unwrap();

        assert_eq!(db.len(), loaded_db.len());
        for id in db.keys() {
            assert!(loaded_db.contains_key(id));
        }

        // Also verify the SQLite db is intact
        let sqlite_db = get_db(file.path()).unwrap();
        assert_eq!(db.len(), sqlite_db.len());
    }

    #[test]
    fn test_maybe_migrate_json() {
        use tempfile::TempDir;

        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join(".carddown");
        std::fs::create_dir_all(&dir).unwrap();
        let db_path = dir.join("carddown.db");

        // Write old-format JSON files
        let entries = get_card_entries();
        let json = serde_json::to_string(&entries).unwrap();
        std::fs::write(dir.join("cards.json"), &json).unwrap();

        let state = GlobalState {
            mean_q: Some(3.5),
            total_cards_revised: 42,
            ..Default::default()
        };
        let state_json = serde_json::to_string(&state).unwrap();
        std::fs::write(dir.join("state.json"), &state_json).unwrap();

        let mut index = ScanIndex::new();
        index.insert("test.md".to_string(), 12345);
        let index_json = serde_json::to_string(&index).unwrap();
        std::fs::write(dir.join("scan_index.json"), &index_json).unwrap();

        // Run migration
        maybe_migrate_json(&db_path).unwrap();
        assert!(db_path.exists());

        // Verify cards migrated
        let migrated_db = get_db(&db_path).unwrap();
        assert_eq!(migrated_db.len(), 2);

        // Verify state migrated
        let migrated_state = get_global_state(&db_path).unwrap();
        assert_eq!(migrated_state.mean_q, Some(3.5));
        assert_eq!(migrated_state.total_cards_revised, 42);

        // Verify scan index migrated
        let migrated_index = load_scan_index(&db_path);
        assert_eq!(migrated_index.get("test.md"), Some(&12345));

        // Old JSON files should still exist
        assert!(dir.join("cards.json").exists());
    }

    #[test]
    fn test_maybe_migrate_json_noop_when_db_exists() {
        use tempfile::TempDir;

        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join(".carddown");
        std::fs::create_dir_all(&dir).unwrap();
        let db_path = dir.join("carddown.db");

        // Create an existing SQLite db with one card
        let mut db = CardDb::new();
        let entries = get_card_entries();
        db.insert(entries[0].card.id, entries[0].clone());
        write_db(&db_path, &db).unwrap();

        // Write JSON with different data
        let json = serde_json::to_string(&get_card_entries()).unwrap();
        std::fs::write(dir.join("cards.json"), &json).unwrap();

        // Migration should be a no-op since db already exists
        maybe_migrate_json(&db_path).unwrap();

        let result_db = get_db(&db_path).unwrap();
        assert_eq!(result_db.len(), 1); // Should not have migrated the second card
    }

    #[test]
    fn test_maybe_migrate_json_noop_when_no_json() {
        use tempfile::TempDir;

        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join(".carddown");
        std::fs::create_dir_all(&dir).unwrap();
        let db_path = dir.join("carddown.db");

        // No JSON files, no db — should be a no-op
        maybe_migrate_json(&db_path).unwrap();
        assert!(!db_path.exists());
    }
}
