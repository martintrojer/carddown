# TODO

## Usability

- [ ] Add short CLI aliases: `-n` for `--maximum-cards-per-session`, `-d` for `--maximum-duration-of-session`, etc.
- [ ] Show user-visible errors when database writes fail during revise/audit exit (currently only `log::error`)
- [ ] Add a summary line after scan ("Found 4 cards, 2 new, 1 updated")
- [ ] Add a session summary after revise ("Reviewed 12 cards, mean quality 3.8")
- [ ] Show card count and due count in `carddown revise` before starting the TUI

## Testing

- [ ] Add integration tests — scan a fixture, revise cards, verify JSON state end-to-end
- [ ] Create dedicated test fixtures instead of sharing `tests/test.md` across tests

## Code

- [ ] Change `Paths` struct fields from `String` to `PathBuf` — blocked by clap `default_value` requiring `&str`
- [ ] Surface database errors to the user in the TUI (e.g., status message on write failure)
