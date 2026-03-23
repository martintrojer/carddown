# TODO

## Code Issues

### Critical

- [ ] Tighten `ONE_LINE_CARD_RE` in `card.rs:11` -- `^(.*):(.*)` matches any line with a colon, relies on fragile upstream guard

### Recommended

- [ ] Change `Paths` struct fields from `String` to `PathBuf` (`main.rs:58-64`) -- blocked by clap `default_value` requiring `&str`
- [ ] Replace 8-param `App::new` with a config struct to eliminate `#[allow(clippy::too_many_arguments)]`
- [ ] Show UI feedback when leech card deletion is blocked in audit mode (currently only logs)

## Test Issues

### Critical -- False Confidence

- [ ] `test_card` (`card.rs:253`) -- asserts values it just assigned; tests nothing about application behavior
- [ ] `test_reverse_probability` (`revise.rs:633`) -- sets `reverse_probability` after construction so `reverse_map` is already computed with `0.0`; doesn't test reversal
- [ ] `test_tag_filtering` / `test_multiple_tags` (`revise.rs:723, 858`) -- set `app.tags` after construction but filtering happens in `filter_cards` before cards reach App
- [ ] `test_concurrent_database_access` (`db.rs:662`) -- only asserts "at least one write succeeded"; doesn't detect corruption or lost writes
- [ ] `test_concurrent_card_updates` (`db.rs:481`) -- named "concurrent" but tests sequential HashMap insertion

### Recommended -- Coverage Gaps

- [ ] Add card content hash stability tests -- pin expected blake3 hashes to detect silent orphaning on refactors
- [ ] Add tests for `formatting.rs` -- currently zero tests; `format_tags` output is nondeterministic from HashSet
- [ ] Fix `test_quality_inputs` (`revise.rs:544`) -- `_expected_quality` is unused; pressing '0' and '5' produce identical results
- [ ] Add property-based algorithm tests verifying monotonic interval growth across quality grades

### Suggestions

- [ ] Extract shared test helpers into a `test_utils` module to reduce duplication across `main.rs`, `db.rs`, `audit.rs`, `revise.rs`
- [ ] Extract filter test boilerplate in `main.rs` into a helper function
- [ ] Fix `test_parse_cards_from_folder` brittleness -- asserts exact count from shared fixture file
- [ ] Remove near-duplicate revise.rs tests (`test_update_state_quality` ~ `test_card_state_updates`, etc.)
- [ ] Add edge case tests for prompts/answers containing `#`, `:`, or the brain emoji in non-marker positions
- [ ] Add test for audit deletion error recovery path (when `delete_fn` returns `Err`)
