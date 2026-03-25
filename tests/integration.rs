use std::path::PathBuf;
use std::process::Command;
use tempfile::TempDir;

fn carddown() -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_carddown"));
    cmd.env("RUST_LOG", "off");
    cmd
}

fn setup_vault(fixture_dir: &str) -> TempDir {
    let tmp = TempDir::new().unwrap();
    let fixtures = PathBuf::from(fixture_dir);
    for entry in std::fs::read_dir(&fixtures).unwrap() {
        let entry = entry.unwrap();
        let dest = tmp.path().join(entry.file_name());
        std::fs::copy(entry.path(), &dest).unwrap();
    }
    std::fs::create_dir_all(tmp.path().join(".carddown")).unwrap();
    tmp
}

fn db_path(vault: &TempDir) -> PathBuf {
    vault.path().join(".carddown/carddown.db")
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

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Found 4 card(s)"), "stderr: {stderr}");

    // Verify database was created
    assert!(db_path(&vault).exists());
}

#[test]
fn test_scan_incremental_skips_unchanged() {
    let vault = setup_vault("tests/fixtures");
    let vault_path = vault.path().to_string_lossy().to_string();

    let output = carddown()
        .args(["--vault", &vault_path])
        .args(["scan", &vault_path])
        .output()
        .unwrap();
    assert!(output.status.success());

    let output = carddown()
        .args(["--vault", &vault_path])
        .args(["scan", &vault_path])
        .output()
        .unwrap();
    assert!(output.status.success());
}

#[test]
fn test_scan_full_detects_orphans() {
    let vault = setup_vault("tests/fixtures");
    let vault_path = vault.path().to_string_lossy().to_string();

    carddown()
        .args(["--vault", &vault_path])
        .args(["scan", "--full", &vault_path])
        .output()
        .unwrap();

    std::fs::remove_file(vault.path().join("single_line.md")).unwrap();

    let output = carddown()
        .args(["--vault", &vault_path])
        .args(["scan", "--full", &vault_path])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("orphaned"), "stderr: {stderr}");
}

#[test]
fn test_revise_no_cards_due() {
    let vault = setup_vault("tests/fixtures");
    let vault_path = vault.path().to_string_lossy().to_string();

    carddown()
        .args(["--vault", &vault_path])
        .args(["scan", &vault_path])
        .output()
        .unwrap();

    let output = carddown()
        .args(["--vault", &vault_path])
        .args(["revise", "-n", "0"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("No cards due") || stderr.contains("0 card(s)"),
        "stderr: {stderr}"
    );
}

#[test]
fn test_revise_with_tag_filter() {
    let vault = setup_vault("tests/fixtures");
    let vault_path = vault.path().to_string_lossy().to_string();

    carddown()
        .args(["--vault", &vault_path])
        .args(["scan", &vault_path])
        .output()
        .unwrap();

    let output = carddown()
        .args(["--vault", &vault_path])
        .args(["revise", "-t", "nonexistent"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("No cards due"), "stderr: {stderr}");
}

#[test]
fn test_import_from_json() {
    let vault = setup_vault("tests/fixtures");
    let vault_path = vault.path().to_string_lossy().to_string();

    // Scan to create the database
    carddown()
        .args(["--vault", &vault_path])
        .args(["scan", &vault_path])
        .output()
        .unwrap();

    // Export to JSON, modify review counts, then re-import
    let export_dir = vault.path().join("export");
    carddown()
        .args(["--vault", &vault_path])
        .args(["export", &export_dir.to_string_lossy()])
        .output()
        .unwrap();

    let cards_json_path = export_dir.join("cards.json");
    let content = std::fs::read_to_string(&cards_json_path).unwrap();
    let mut cards: Vec<serde_json::Value> = serde_json::from_str(&content).unwrap();
    for card in &mut cards {
        card["revise_count"] = serde_json::json!(5);
    }
    let source_path = vault.path().join("source.json");
    std::fs::write(&source_path, serde_json::to_string(&cards).unwrap()).unwrap();

    // Import from JSON
    let output = carddown()
        .args(["--vault", &vault_path])
        .args(["import", &source_path.to_string_lossy()])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Imported"), "stderr: {stderr}");
}

#[test]
fn test_import_from_sqlite() {
    let vault1 = setup_vault("tests/fixtures");
    let vault2 = setup_vault("tests/fixtures");
    let v1_path = vault1.path().to_string_lossy().to_string();
    let v2_path = vault2.path().to_string_lossy().to_string();

    // Scan both vaults
    carddown()
        .args(["--vault", &v1_path])
        .args(["scan", &v1_path])
        .output()
        .unwrap();
    carddown()
        .args(["--vault", &v2_path])
        .args(["scan", &v2_path])
        .output()
        .unwrap();

    // Import vault2's db into vault1 (same cards, so 0 updates since both at revise_count=0)
    let output = carddown()
        .args(["--vault", &v1_path])
        .args(["import", &db_path(&vault2).to_string_lossy()])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("0 card(s)"), "stderr: {stderr}");
}

#[test]
fn test_vault_isolation() {
    let vault1 = setup_vault("tests/fixtures");
    let vault2 = TempDir::new().unwrap();
    std::fs::create_dir_all(vault2.path().join(".carddown")).unwrap();

    let v1_path = vault1.path().to_string_lossy().to_string();
    let v2_path = vault2.path().to_string_lossy().to_string();

    carddown()
        .args(["--vault", &v1_path])
        .args(["scan", &v1_path])
        .output()
        .unwrap();

    // Vault2 should have no database
    assert!(!db_path(&vault2).exists());

    carddown()
        .args(["--vault", &v2_path])
        .args(["scan", "--full", &v2_path])
        .output()
        .unwrap();

    // Vault1 db should still exist and be intact
    assert!(db_path(&vault1).exists());
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
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("[dry-run]"), "stderr: {stderr}");
    assert!(stderr.contains("4 new"), "stderr: {stderr}");

    assert!(
        !db_path(&vault).exists(),
        "dry-run should not create database"
    );
}

#[test]
fn test_import_dry_run_does_not_write() {
    let vault = setup_vault("tests/fixtures");
    let vault_path = vault.path().to_string_lossy().to_string();

    carddown()
        .args(["--vault", &vault_path])
        .args(["scan", &vault_path])
        .output()
        .unwrap();

    // Export, modify, create source JSON
    let export_dir = vault.path().join("export");
    carddown()
        .args(["--vault", &vault_path])
        .args(["export", &export_dir.to_string_lossy()])
        .output()
        .unwrap();
    let content = std::fs::read_to_string(export_dir.join("cards.json")).unwrap();
    let mut cards: Vec<serde_json::Value> = serde_json::from_str(&content).unwrap();
    for card in &mut cards {
        card["revise_count"] = serde_json::json!(10);
    }
    let source_path = vault.path().join("source.json");
    std::fs::write(&source_path, serde_json::to_string(&cards).unwrap()).unwrap();

    let output = carddown()
        .args(["--vault", &vault_path])
        .args(["import", "--dry-run", &source_path.to_string_lossy()])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("[dry-run]"), "stderr: {stderr}");

    // Verify db wasn't modified by exporting again
    let verify_dir = vault.path().join("verify");
    carddown()
        .args(["--vault", &vault_path])
        .args(["export", &verify_dir.to_string_lossy()])
        .output()
        .unwrap();
    let verify_content = std::fs::read_to_string(verify_dir.join("cards.json")).unwrap();
    let verify_cards: Vec<serde_json::Value> = serde_json::from_str(&verify_content).unwrap();
    for card in &verify_cards {
        assert_eq!(
            card["revise_count"], 0,
            "dry-run should not modify database"
        );
    }
}

#[test]
fn test_export() {
    let vault = setup_vault("tests/fixtures");
    let vault_path = vault.path().to_string_lossy().to_string();

    carddown()
        .args(["--vault", &vault_path])
        .args(["scan", &vault_path])
        .output()
        .unwrap();

    let export_dir = vault.path().join("export");
    let output = carddown()
        .args(["--vault", &vault_path])
        .args(["export", &export_dir.to_string_lossy()])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Exported 4 card(s)"), "stderr: {stderr}");

    assert!(export_dir.join("cards.json").exists());
    assert!(export_dir.join("state.json").exists());

    let content = std::fs::read_to_string(export_dir.join("cards.json")).unwrap();
    let cards: Vec<serde_json::Value> = serde_json::from_str(&content).unwrap();
    assert_eq!(cards.len(), 4);
}

#[test]
fn test_auto_migrate_from_json() {
    let vault = setup_vault("tests/fixtures");
    let vault_path = vault.path().to_string_lossy().to_string();

    // First scan with the current version to get valid JSON data
    carddown()
        .args(["--vault", &vault_path])
        .args(["scan", &vault_path])
        .output()
        .unwrap();

    // Export to get JSON cards
    let export_dir = vault.path().join("export");
    carddown()
        .args(["--vault", &vault_path])
        .args(["export", &export_dir.to_string_lossy()])
        .output()
        .unwrap();

    // Simulate a pre-0.4.0 vault: delete the SQLite db, put JSON files in .carddown/
    let carddown_dir = vault.path().join(".carddown");
    std::fs::remove_file(carddown_dir.join("carddown.db")).unwrap();
    std::fs::copy(
        export_dir.join("cards.json"),
        carddown_dir.join("cards.json"),
    )
    .unwrap();
    std::fs::copy(
        export_dir.join("state.json"),
        carddown_dir.join("state.json"),
    )
    .unwrap();

    // Any command should trigger auto-migration
    let output = carddown()
        .args(["--vault", &vault_path])
        .args(["revise", "-n", "0"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Migrating from JSON to SQLite"),
        "stderr: {stderr}"
    );

    // Verify the SQLite db was created and has the cards
    assert!(carddown_dir.join("carddown.db").exists());

    // Export again and verify data integrity
    let verify_dir = vault.path().join("verify");
    carddown()
        .args(["--vault", &vault_path])
        .args(["export", &verify_dir.to_string_lossy()])
        .output()
        .unwrap();
    let content = std::fs::read_to_string(verify_dir.join("cards.json")).unwrap();
    let cards: Vec<serde_json::Value> = serde_json::from_str(&content).unwrap();
    assert_eq!(cards.len(), 4);
}
