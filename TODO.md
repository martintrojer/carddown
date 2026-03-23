# TODO

## Code Issues

- [ ] Change `Paths` struct fields from `String` to `PathBuf` (`main.rs:58-64`) -- blocked by clap `default_value` requiring `&str`

## Test Suggestions

- [ ] Extract shared test helpers into a `test_utils` module to reduce duplication across `main.rs`, `db.rs`, `audit.rs`, `revise.rs`
- [ ] Extract filter test boilerplate in `main.rs` into a helper function
- [ ] Fix `test_parse_cards_from_folder` brittleness -- asserts exact count from shared fixture file
