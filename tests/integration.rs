use std::path::PathBuf;
use std::process::Command;
use tempfile::TempDir;

fn carddown() -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_carddown"));
    // Silence log output in tests
    cmd.env("RUST_LOG", "off");
    cmd
}

fn setup_vault(fixture_dir: &str) -> TempDir {
    let tmp = TempDir::new().unwrap();
    // Copy fixture files into temp dir
    let fixtures = PathBuf::from(fixture_dir);
    for entry in std::fs::read_dir(&fixtures).unwrap() {
        let entry = entry.unwrap();
        let dest = tmp.path().join(entry.file_name());
        std::fs::copy(entry.path(), &dest).unwrap();
    }
    // Create .carddown dir so vault discovery finds it
    std::fs::create_dir_all(tmp.path().join(".carddown")).unwrap();
    tmp
}

#[test]
fn test_scan_creates_database() {
    let vault = setup_vault("tests/fixtures");

    let output = carddown()
        .args(["--vault", &vault.path().to_string_lossy()])
        .args(["scan", &vault.path().to_string_lossy()])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Found 4 card(s)"), "stdout: {stdout}");

    // Verify database was created
    let db_path = vault.path().join(".carddown/cards.json");
    assert!(db_path.exists());
    let db_content = std::fs::read_to_string(&db_path).unwrap();
    let cards: Vec<serde_json::Value> = serde_json::from_str(&db_content).unwrap();
    assert_eq!(cards.len(), 4);
}

#[test]
fn test_scan_incremental_skips_unchanged() {
    let vault = setup_vault("tests/fixtures");
    let vault_path = vault.path().to_string_lossy().to_string();

    // First scan
    let output = carddown()
        .args(["--vault", &vault_path])
        .args(["scan", &vault_path])
        .output()
        .unwrap();
    assert!(output.status.success());

    // Second scan — no changes, should skip
    let output = carddown()
        .args(["--vault", &vault_path])
        .args(["scan", &vault_path])
        .output()
        .unwrap();
    assert!(output.status.success());
    // Incremental scan with no changes doesn't print anything (exits early)
}

#[test]
fn test_scan_full_detects_orphans() {
    let vault = setup_vault("tests/fixtures");
    let vault_path = vault.path().to_string_lossy().to_string();

    // Initial full scan
    carddown()
        .args(["--vault", &vault_path])
        .args(["scan", "--full", &vault_path])
        .output()
        .unwrap();

    // Remove a fixture file
    std::fs::remove_file(vault.path().join("single_line.md")).unwrap();

    // Full scan again — should detect orphans
    let output = carddown()
        .args(["--vault", &vault_path])
        .args(["scan", "--full", &vault_path])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("orphaned"), "stdout: {stdout}");
}

#[test]
fn test_revise_no_cards_due() {
    let vault = setup_vault("tests/fixtures");
    let vault_path = vault.path().to_string_lossy().to_string();

    // Scan first
    carddown()
        .args(["--vault", &vault_path])
        .args(["scan", &vault_path])
        .output()
        .unwrap();

    // Revise with 0 max cards — should say no cards (all cards are new and due,
    // but we limit to 0)
    let output = carddown()
        .args(["--vault", &vault_path])
        .args(["revise", "-n", "0"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("No cards due") || stdout.contains("0 card(s)"),
        "stdout: {stdout}"
    );
}

#[test]
fn test_revise_with_tag_filter() {
    let vault = setup_vault("tests/fixtures");
    let vault_path = vault.path().to_string_lossy().to_string();

    // Scan
    carddown()
        .args(["--vault", &vault_path])
        .args(["scan", &vault_path])
        .output()
        .unwrap();

    // Revise with non-existent tag — no cards
    let output = carddown()
        .args(["--vault", &vault_path])
        .args(["revise", "-t", "nonexistent"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("No cards due"), "stdout: {stdout}");
}

#[test]
fn test_import_merges_stats() {
    let vault = setup_vault("tests/fixtures");
    let vault_path = vault.path().to_string_lossy().to_string();

    // Scan to create the database
    carddown()
        .args(["--vault", &vault_path])
        .args(["scan", &vault_path])
        .output()
        .unwrap();

    // Copy the db as a "source" and modify it to have review history
    let db_path = vault.path().join(".carddown/cards.json");
    let source_path = vault.path().join("source_cards.json");
    let db_content = std::fs::read_to_string(&db_path).unwrap();
    let mut cards: Vec<serde_json::Value> = serde_json::from_str(&db_content).unwrap();
    for card in &mut cards {
        card["revise_count"] = serde_json::json!(5);
    }
    std::fs::write(&source_path, serde_json::to_string(&cards).unwrap()).unwrap();

    // Import
    let output = carddown()
        .args(["--vault", &vault_path])
        .args(["import", &source_path.to_string_lossy()])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Imported"), "stdout: {stdout}");

    // Verify stats were imported
    let result_content = std::fs::read_to_string(&db_path).unwrap();
    let result_cards: Vec<serde_json::Value> = serde_json::from_str(&result_content).unwrap();
    for card in &result_cards {
        assert_eq!(card["revise_count"], 5);
    }
}

#[test]
fn test_vault_isolation() {
    let vault1 = setup_vault("tests/fixtures");
    let vault2 = TempDir::new().unwrap();
    std::fs::create_dir_all(vault2.path().join(".carddown")).unwrap();

    let v1_path = vault1.path().to_string_lossy().to_string();
    let v2_path = vault2.path().to_string_lossy().to_string();

    // Scan vault1
    carddown()
        .args(["--vault", &v1_path])
        .args(["scan", &v1_path])
        .output()
        .unwrap();

    // Vault2 should have no cards
    let db2_path = vault2.path().join(".carddown/cards.json");
    assert!(!db2_path.exists());

    // Full scan of empty vault2 should not affect vault1
    carddown()
        .args(["--vault", &v2_path])
        .args(["scan", "--full", &v2_path])
        .output()
        .unwrap();

    // Vault1 cards should still be intact
    let db1_path = vault1.path().join(".carddown/cards.json");
    let content = std::fs::read_to_string(&db1_path).unwrap();
    let cards: Vec<serde_json::Value> = serde_json::from_str(&content).unwrap();
    assert_eq!(cards.len(), 4);
}

#[test]
fn test_scan_dry_run_does_not_write() {
    let vault = setup_vault("tests/fixtures");
    let vault_path = vault.path().to_string_lossy().to_string();

    let output = carddown()
        .args(["--vault", &vault_path])
        .args(["scan", "--dry-run", &vault_path])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("[dry-run]"), "stdout: {stdout}");
    assert!(stdout.contains("4 new"), "stdout: {stdout}");

    // Database should NOT exist
    let db_path = vault.path().join(".carddown/cards.json");
    assert!(!db_path.exists(), "dry-run should not create database");
}

#[test]
fn test_import_dry_run_does_not_write() {
    let vault = setup_vault("tests/fixtures");
    let vault_path = vault.path().to_string_lossy().to_string();

    // Scan first to create the database
    carddown()
        .args(["--vault", &vault_path])
        .args(["scan", &vault_path])
        .output()
        .unwrap();

    // Create source with review history
    let db_path = vault.path().join(".carddown/cards.json");
    let source_path = vault.path().join("source.json");
    let db_content = std::fs::read_to_string(&db_path).unwrap();
    let mut cards: Vec<serde_json::Value> = serde_json::from_str(&db_content).unwrap();
    for card in &mut cards {
        card["revise_count"] = serde_json::json!(10);
    }
    std::fs::write(&source_path, serde_json::to_string(&cards).unwrap()).unwrap();

    // Dry-run import
    let output = carddown()
        .args(["--vault", &vault_path])
        .args(["import", "--dry-run", &source_path.to_string_lossy()])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("[dry-run]"), "stdout: {stdout}");

    // Database should NOT have been modified
    let result_content = std::fs::read_to_string(&db_path).unwrap();
    let result_cards: Vec<serde_json::Value> = serde_json::from_str(&result_content).unwrap();
    for card in &result_cards {
        assert_eq!(
            card["revise_count"], 0,
            "dry-run should not modify database"
        );
    }
}
