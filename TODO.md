# TODO

## Code Issues

### Critical

- [ ] Tighten `ONE_LINE_CARD_RE` in `card.rs:11` -- `^(.*):(.*)` matches any line with a colon, relies on fragile upstream guard

### Recommended

- [ ] Change `Paths` struct fields from `String` to `PathBuf` (`main.rs:58-64`) -- blocked by clap `default_value` requiring `&str`
- [ ] Replace 8-param `App::new` with a config struct to eliminate `#[allow(clippy::too_many_arguments)]`
- [ ] Show UI feedback when leech card deletion is blocked in audit mode (currently only logs)

## Test Issues

### Suggestions

- [ ] Extract shared test helpers into a `test_utils` module to reduce duplication across `main.rs`, `db.rs`, `audit.rs`, `revise.rs`
- [ ] Extract filter test boilerplate in `main.rs` into a helper function
- [ ] Fix `test_parse_cards_from_folder` brittleness -- asserts exact count from shared fixture file
- [ ] Remove near-duplicate revise.rs tests (`test_update_state_quality` ~ `test_card_state_updates`, etc.)
- [ ] Add edge case tests for prompts/answers containing `#`, `:`, or the brain emoji in non-marker positions
- [ ] Add test for audit deletion error recovery path (when `delete_fn` returns `Err`)
- [ ] Add property-based algorithm tests verifying monotonic interval growth across quality grades
