use crate::algorithm::{update_meanq, Algorithm, Quality};
use crate::view::formatting::{format_datetime_opt, format_tags};
use anyhow::Result;
use rand::Rng;
use ratatui::prelude::*;
use std::io;
use std::time::{Duration, Instant};

use crate::db::{CardEntry, GlobalState};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind};
use ratatui::{
    symbols::border,
    widgets::{block::*, *},
};

struct UiState {
    current_card: usize,
    exit: bool,
    help: bool,
    revealed: bool,
    started: Instant,
}

pub struct ReviseConfig {
    pub leech_threshold: usize,
    pub max_duration: usize,
    pub reverse_probability: f64,
    pub tags: Vec<String>,
}

pub struct App {
    algorithm: Box<dyn Algorithm>,
    cards: Vec<CardEntry>,
    global_state: GlobalState,
    config: ReviseConfig,
    // Whether each card should be reversed for this session
    reverse_map: Vec<bool>,
    #[allow(clippy::type_complexity)]
    update_fn: Box<dyn Fn(Vec<CardEntry>, &GlobalState) -> Result<()>>,
    ui: UiState,
}

impl App {
    #[allow(clippy::type_complexity)]
    pub fn new(
        algorithm: Box<dyn Algorithm>,
        cards: Vec<CardEntry>,
        global_state: GlobalState,
        config: ReviseConfig,
        update_fn: Box<dyn Fn(Vec<CardEntry>, &GlobalState) -> Result<()>>,
    ) -> Self {
        let mut rng = rand::rng();
        let reverse_map = (0..cards.len())
            .map(|_| rng.random::<f64>() < config.reverse_probability)
            .collect();
        Self {
            algorithm,
            cards,
            update_fn,
            global_state,
            config,
            reverse_map,
            ui: UiState {
                current_card: 0,
                exit: false,
                help: false,
                revealed: false,
                started: Instant::now(),
            },
        }
    }

    /// runs the application's main loop until the user quits
    pub fn run(&mut self, terminal: &mut super::Tui) -> io::Result<()> {
        while !self.ui.exit {
            terminal.draw(|frame| self.render_frame(frame))?;
            self.handle_events()?;
        }
        Ok(())
    }

    fn render_frame(&self, frame: &mut Frame) {
        frame.render_widget(self, frame.area());
    }

    /// updates the application's state based on user input
    fn handle_events(&mut self) -> io::Result<()> {
        if let Ok(true) = event::poll(Duration::from_secs(1)) {
            if self.is_session_expired() {
                self.exit();
            }
            match event::read()? {
                // it's important to check that the event is a key press event as
                // crossterm also emits key release and repeat events on Windows.
                Event::Key(key_event) if key_event.kind == KeyEventKind::Press => {
                    self.handle_key_event(key_event)
                }
                _ => {}
            };
        }
        Ok(())
    }

    fn update_state(&mut self, quality: Quality) {
        self.ui.revealed = false;

        if self.cards.is_empty() {
            return;
        }

        let current_card = self.ui.current_card;

        // Check if we've reached the end of the card list
        if current_card >= self.cards.len() {
            self.exit();
            return;
        }

        // Update global statistics
        update_meanq(&mut self.global_state, quality);

        // Update the current card's state
        if let Some(card) = self.cards.get_mut(current_card) {
            card.last_revised = Some(chrono::Utc::now());
            card.revise_count += 1;
            self.algorithm
                .update_state(&quality, &mut card.state, &mut self.global_state);

            // Check if card should be marked as leech
            if card.state.failed_count >= self.config.leech_threshold as u64 {
                card.leech = true;
            }
        }

        // Move to next card
        self.ui.current_card += 1;
    }

    fn handle_key_event(&mut self, key_event: KeyEvent) {
        match key_event.code {
            KeyCode::Char('q') | KeyCode::Char('Q') => {
                if self.ui.help {
                    self.ui.help = false;
                } else {
                    self.exit();
                }
            }
            KeyCode::Char(' ') if !self.ui.help => self.ui.revealed = true,
            KeyCode::Char('?') => self.ui.help = !self.ui.help,
            KeyCode::Char('0') | KeyCode::Char('a') if !self.ui.help => {
                self.update_state(Quality::IncorrectAndForgotten)
            }
            KeyCode::Char('1') | KeyCode::Char('d') if !self.ui.help => {
                self.update_state(Quality::IncorrectButRemembered)
            }
            KeyCode::Char('2') | KeyCode::Char('g') if !self.ui.help => {
                self.update_state(Quality::IncorrectButEasyToRecall)
            }
            KeyCode::Char('3') | KeyCode::Char('j') if !self.ui.help => {
                self.update_state(Quality::CorrectWithDifficulty)
            }
            KeyCode::Char('4') | KeyCode::Char('l') if !self.ui.help => {
                self.update_state(Quality::CorrectWithHesitation)
            }
            KeyCode::Char('5') | KeyCode::Char('\'') if !self.ui.help => {
                self.update_state(Quality::Perfect)
            }
            _ => {}
        }
    }

    fn is_session_expired(&self) -> bool {
        self.ui.started.elapsed().as_secs() >= self.config.max_duration as u64 * 60
    }

    fn exit(&mut self) {
        if let Err(e) = (self.update_fn)(std::mem::take(&mut self.cards), &self.global_state) {
            log::error!("Failed to update cards during exit: {e}");
        }
        self.ui.exit = true;
    }

    fn help(&self) -> (Block<'_>, Text<'_>) {
        let title = Line::from(" Key Bindings ".bold());
        let secs = self.ui.started.elapsed().as_secs();
        let min = secs / 60;
        let secs = secs % 60;
        let instructions = Line::from(vec![
            " Quit ".into(),
            "<Q> ".bold(),
            "Elapsed ".into(),
            format!("{min}:{secs:02} ").bold(),
        ]);
        let block = Block::default()
            .title(title)
            .title_bottom(instructions)
            .title_alignment(Alignment::Center)
            .borders(Borders::ALL)
            .border_set(border::ROUNDED);

        let counter_text = Text::from(vec![
            Line::from(vec![]),
            Line::from(vec!["Qualities".into()]),
            Line::from(vec![]),
            Line::from(vec![format!(
                "{} or {}: {:?}",
                Quality::IncorrectAndForgotten as u8,
                'a',
                Quality::IncorrectAndForgotten
            )
            .red()]),
            Line::from(vec![format!(
                "{} or {}: {:?}",
                Quality::IncorrectButRemembered as u8,
                'd',
                Quality::IncorrectButRemembered
            )
            .red()]),
            Line::from(vec![format!(
                "{} or {}: {:?}",
                Quality::IncorrectButEasyToRecall as u8,
                'g',
                Quality::IncorrectButEasyToRecall
            )
            .red()]),
            Line::from(vec![format!(
                "{} or {}: {:?}",
                Quality::CorrectWithDifficulty as u8,
                'j',
                Quality::CorrectWithDifficulty
            )
            .yellow()]),
            Line::from(vec![format!(
                "{} or {}: {:?}",
                Quality::CorrectWithHesitation as u8,
                'l',
                Quality::CorrectWithHesitation
            )
            .yellow()]),
            Line::from(vec![format!(
                "{} or {}: {:?}",
                Quality::Perfect as u8,
                '\'',
                Quality::Perfect
            )
            .green()]),
        ]);
        (block, counter_text)
    }

    fn card_revise(&self) -> (Block<'_>, Text<'_>) {
        let reversed = self
            .reverse_map
            .get(self.ui.current_card)
            .copied()
            .unwrap_or(false);
        let title = Line::from(
            format!(
                " {} Revise Cards {}/{} [{} | algo:{} | rev:{:.2}] ",
                if reversed { "[Reversed]" } else { "" },
                std::cmp::min(self.cards.len(), 1 + self.ui.current_card),
                self.cards.len(),
                if self.config.tags.is_empty() {
                    "All Tags".to_string()
                } else {
                    self.config.tags.join(", ")
                },
                self.algorithm.name(),
                self.config.reverse_probability,
            )
            .bold(),
        );
        let secs = self.ui.started.elapsed().as_secs();
        let min = secs / 60;
        let secs = secs % 60;
        let instructions = Line::from(vec![
            " Quit ".into(),
            "<Q> ".bold(),
            "Reveal ".into(),
            "<Space> ".blue().bold(),
            "Score/Quality ".into(),
            "<0-5> ".green().bold(),
            "Help ".into(),
            "<?> ".bold(),
            "Elapsed ".into(),
            format!("{min}:{secs:02} ").bold(),
            // algorithm printed in title; keep instruction compact
        ]);
        let block = Block::default()
            .title(title)
            .title_bottom(instructions)
            .title_alignment(Alignment::Center)
            .borders(Borders::ALL)
            .border_set(border::ROUNDED);
        // 'reversed' already computed above; keep a local binding in scope
        let counter_text = if self.cards.is_empty() || self.ui.current_card >= self.cards.len() {
            Text::from(vec![Line::from(vec!["No cards to revise".into()])])
        } else {
            let card = &self.cards[self.ui.current_card];
            let mut lines: Vec<Line> = Vec::new();
            lines.push(if card.leech {
                Line::from(vec!["Leech Card".red().bold()])
            } else if card.orphan {
                Line::from(vec!["Orphan Card".yellow().bold()])
            } else {
                Line::from(vec!["".into()])
            });
            lines.push(Line::from(vec![]));
            // Tags
            if !card.card.tags.is_empty() {
                lines.push(Line::from(vec!["Tags".bold()]));
                lines.push(Line::from(vec![format_tags(&card.card.tags).into()]));
                lines.push(Line::from(vec![]));
            }
            if !reversed {
                lines.push(Line::from(vec!["Prompt".bold()]));
                lines.push(Line::from(vec![card.card.prompt.clone().into()]));
                lines.push(Line::from(vec![]));
                lines.push(Line::from(vec!["Response".bold()]));
                if self.ui.revealed {
                    for l in card.card.response.iter() {
                        lines.push(Line::from(vec![l.into()]));
                    }
                } else {
                    lines.push(Line::from(vec!["<hidden>".into()]));
                }
            } else {
                // Reversed: show response as the prompt; hide the original prompt until reveal
                lines.push(Line::from(vec!["Prompt".bold()]));
                for l in card.card.response.iter() {
                    lines.push(Line::from(vec![l.into()]));
                }
                lines.push(Line::from(vec![]));
                lines.push(Line::from(vec!["Response".bold()]));
                if self.ui.revealed {
                    lines.push(Line::from(vec![card.card.prompt.clone().into()]));
                } else {
                    lines.push(Line::from(vec!["<hidden>".into()]));
                }
            }
            lines.push(Line::from(vec![]));
            lines.push(Line::from(vec!["Last Revised".bold()]));
            lines.push(Line::from(vec![format_datetime_opt(
                card.last_revised,
                "Never",
            )
            .into()]));
            Text::from(lines)
        };

        (block, counter_text)
    }
}

impl Widget for &App {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let (block, counter_text) = if self.ui.help {
            self.help()
        } else {
            self.card_revise()
        };
        Paragraph::new(counter_text)
            .wrap(Wrap { trim: true })
            .centered()
            .block(block)
            .render(area, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::algorithm::new_algorithm;
    use crate::algorithm::Algo;
    use crate::card::Card;
    use std::collections::HashSet;
    use std::path::PathBuf;

    fn create_test_app() -> App {
        let algorithm = new_algorithm(Algo::SM2);
        let card = Card {
            id: blake3::hash(b"test"),
            file: PathBuf::from("test.md"),
            line: 0,
            prompt: "test prompt".to_string(),
            response: vec!["test response".to_string()],
            tags: HashSet::new(),
        };
        let cards = vec![CardEntry {
            added: chrono::Utc::now(),
            card,
            last_revised: None,
            revise_count: 0,
            state: Default::default(),
            leech: false,
            orphan: false,
        }];
        let global_state = GlobalState::default();
        fn update_fn(_cards: Vec<CardEntry>, _state: &GlobalState) -> Result<()> {
            Ok(())
        }
        App::new(
            algorithm,
            cards,
            global_state,
            ReviseConfig {
                leech_threshold: 3,
                max_duration: 3600,
                reverse_probability: 0.0,
                tags: vec![],
            },
            Box::new(update_fn),
        )
    }

    fn refresh_global_state(state: &mut GlobalState) {
        state.last_revise_session = Some(chrono::Utc::now());
        state.mean_q = Some(0.0);
        state.total_cards_revised = 0;
    }

    #[test]
    fn test_leech_threshold() {
        let mut app = create_test_app();

        // Fail the card enough times to trigger leech status
        for _ in 0..3 {
            app.ui.current_card = 0; // Reset card index for each attempt
            app.update_state(Quality::IncorrectAndForgotten);
        }

        let card = &app.cards[0];
        assert!(card.leech);
        assert_eq!(card.state.failed_count, 3);
    }

    #[test]
    fn test_handle_key_events() {
        let mut app = create_test_app();

        // Test reveal
        app.handle_key_event(KeyEvent::new(KeyCode::Char(' '), event::KeyModifiers::NONE));
        assert!(app.ui.revealed);

        // Test quality input
        app.handle_key_event(KeyEvent::new(KeyCode::Char('5'), event::KeyModifiers::NONE));
        assert!(!app.ui.revealed);
        assert_eq!(app.ui.current_card, 1);
    }

    #[test]
    fn test_empty_card_list() {
        let mut app = create_test_app();
        app.cards.clear();

        // Should handle empty card list gracefully
        app.update_state(Quality::Perfect);
        assert_eq!(app.ui.current_card, 0);

        // UI should show no cards message
        app.handle_key_event(KeyEvent::new(KeyCode::Char(' '), event::KeyModifiers::NONE));
        assert!(app.ui.revealed);
    }

    #[test]
    fn test_card_navigation() {
        let mut app = create_test_app();
        let second_card = app.cards[0].clone(); // Clone first card for second entry
        app.cards.push(second_card);

        assert_eq!(app.ui.current_card, 0);
        app.update_state(Quality::Perfect);
        assert_eq!(app.ui.current_card, 1);

        // After processing the last card, it should trigger exit
        app.ui.current_card = app.cards.len(); // Simulate reaching end of cards
        app.update_state(Quality::Perfect);
        assert!(app.ui.exit); // Now should exit
    }

    // Alternative version that tests actual navigation
    #[test]
    fn test_card_navigation_full() {
        let mut app = create_test_app();
        let second_card = app.cards[0].clone();
        app.cards.push(second_card);

        // Process first card
        assert_eq!(app.ui.current_card, 0);
        app.handle_key_event(KeyEvent::new(KeyCode::Char('5'), event::KeyModifiers::NONE));
        assert_eq!(app.ui.current_card, 1);

        // Process second (last) card
        app.handle_key_event(KeyEvent::new(KeyCode::Char('5'), event::KeyModifiers::NONE));
        assert_eq!(app.ui.current_card, 2); // Will be at end of cards

        // One more update should trigger exit
        app.update_state(Quality::Perfect);
        assert!(app.ui.exit);
    }

    #[test]
    fn test_help_screen() {
        let mut app = create_test_app();

        // Initially help should be hidden
        assert!(!app.ui.help);

        // Toggle help on
        app.handle_key_event(KeyEvent::new(KeyCode::Char('?'), event::KeyModifiers::NONE));
        assert!(app.ui.help);

        // Regular keys should be ignored when help is shown
        app.handle_key_event(KeyEvent::new(KeyCode::Char(' '), event::KeyModifiers::NONE));
        assert!(!app.ui.revealed);

        // Toggle help off
        app.handle_key_event(KeyEvent::new(KeyCode::Char('?'), event::KeyModifiers::NONE));
        assert!(!app.ui.help);
    }

    #[test]
    fn test_quality_inputs() {
        // Test all quality inputs with both number and letter keys
        let quality_keys = [
            (('0', 'a'), true),   // failure: should increment failed_count
            (('1', 'd'), true),   // failure
            (('2', 'g'), true),   // failure
            (('3', 'j'), false),  // success: should not increment failed_count
            (('4', 'l'), false),  // success
            (('5', '\''), false), // success
        ];

        for ((num_key, letter_key), is_failure) in quality_keys.iter() {
            // Test number key
            let mut app = create_test_app();
            app.handle_key_event(KeyEvent::new(
                KeyCode::Char(*num_key),
                event::KeyModifiers::NONE,
            ));
            assert_eq!(app.ui.current_card, 1);
            if *is_failure {
                assert_eq!(
                    app.cards[0].state.failed_count, 1,
                    "key '{num_key}' should be a failure"
                );
            } else {
                assert_eq!(
                    app.cards[0].state.failed_count, 0,
                    "key '{num_key}' should be a success"
                );
            }

            // Test letter key
            let mut app = create_test_app();
            app.handle_key_event(KeyEvent::new(
                KeyCode::Char(*letter_key),
                event::KeyModifiers::NONE,
            ));
            assert_eq!(app.ui.current_card, 1);
            if *is_failure {
                assert_eq!(
                    app.cards[0].state.failed_count, 1,
                    "key '{letter_key}' should be a failure"
                );
            } else {
                assert_eq!(
                    app.cards[0].state.failed_count, 0,
                    "key '{letter_key}' should be a success"
                );
            }
        }
    }

    #[test]
    fn test_exit_behavior() {
        let mut app = create_test_app();

        // Test normal exit
        app.handle_key_event(KeyEvent::new(KeyCode::Char('q'), event::KeyModifiers::NONE));
        assert!(app.ui.exit);

        // Test exit with help screen open
        let mut app = create_test_app();
        app.ui.help = true;
        app.handle_key_event(KeyEvent::new(KeyCode::Char('q'), event::KeyModifiers::NONE));
        assert!(!app.ui.exit); // Should close help first
        assert!(!app.ui.help);

        app.handle_key_event(KeyEvent::new(KeyCode::Char('q'), event::KeyModifiers::NONE));
        assert!(app.ui.exit); // Now should exit
    }

    #[test]
    fn test_max_duration() {
        let mut app = create_test_app();
        app.config.max_duration = 0; // Set to 0 to test immediate timeout
        assert!(app.is_session_expired());
    }

    #[test]
    fn test_max_duration_uses_minutes_not_seconds() {
        let mut app = create_test_app();
        app.config.max_duration = 1; // 1 minute
                                     // Simulate 30 seconds elapsed -- should NOT be expired
        app.ui.started = Instant::now() - Duration::from_secs(30);
        assert!(!app.is_session_expired());

        // Simulate 61 seconds elapsed -- SHOULD be expired
        app.ui.started = Instant::now() - Duration::from_secs(61);
        assert!(app.is_session_expired());
    }

    #[test]
    fn test_card_state_updates() {
        let mut app = create_test_app();

        // Test that card state is properly updated
        app.update_state(Quality::Perfect);
        let card = &app.cards[0];
        assert!(card.last_revised.is_some());
        assert_eq!(card.revise_count, 1);
        assert!(!card.leech);

        // Test failed count increment
        let mut app = create_test_app();
        app.update_state(Quality::IncorrectAndForgotten);
        let card = &app.cards[0];
        assert_eq!(card.state.failed_count, 1);
    }

    #[test]
    fn test_reverse_probability() {
        // reverse_map is computed at construction time, so we must set
        // reverse_probability before creating the App
        let algorithm = new_algorithm(Algo::SM2);
        let card = Card {
            id: blake3::hash(b"test"),
            file: PathBuf::from("test.md"),
            line: 0,
            prompt: "test prompt".to_string(),
            response: vec!["test response".to_string()],
            tags: HashSet::new(),
        };
        let cards = vec![CardEntry {
            added: chrono::Utc::now(),
            card,
            last_revised: None,
            revise_count: 0,
            state: Default::default(),
            leech: false,
            orphan: false,
        }];
        fn update_fn(_cards: Vec<CardEntry>, _state: &GlobalState) -> Result<()> {
            Ok(())
        }
        let app = App::new(
            algorithm,
            cards,
            GlobalState::default(),
            ReviseConfig {
                leech_threshold: 3,
                max_duration: 3600,
                reverse_probability: 1.0, // Always reverse
                tags: vec![],
            },
            Box::new(update_fn),
        );

        // With probability 1.0, all cards should be reversed
        assert!(app.reverse_map[0]);

        // Verify a 0.0 probability produces no reversals
        let algorithm = new_algorithm(Algo::SM2);
        let card = Card {
            id: blake3::hash(b"test"),
            file: PathBuf::from("test.md"),
            line: 0,
            prompt: "test prompt".to_string(),
            response: vec!["test response".to_string()],
            tags: HashSet::new(),
        };
        let cards = vec![CardEntry {
            added: chrono::Utc::now(),
            card,
            last_revised: None,
            revise_count: 0,
            state: Default::default(),
            leech: false,
            orphan: false,
        }];
        let app = App::new(
            algorithm,
            cards,
            GlobalState::default(),
            ReviseConfig {
                leech_threshold: 3,
                max_duration: 3600,
                reverse_probability: 0.0, // Never reverse
                tags: vec![],
            },
            Box::new(update_fn),
        );
        assert!(!app.reverse_map[0]);
    }

    #[test]
    fn test_card_reveal_state() {
        let mut app = create_test_app();

        // Initially card should not be revealed
        assert!(!app.ui.revealed);

        // Space should reveal card
        app.handle_key_event(KeyEvent::new(KeyCode::Char(' '), event::KeyModifiers::NONE));
        assert!(app.ui.revealed);

        // Quality input should hide card again
        app.handle_key_event(KeyEvent::new(KeyCode::Char('5'), event::KeyModifiers::NONE));
        assert!(!app.ui.revealed);
    }

    #[test]
    fn test_invalid_quality_inputs() {
        let mut app = create_test_app();
        let initial_card = app.ui.current_card;

        // Invalid quality inputs should be ignored
        app.handle_key_event(KeyEvent::new(KeyCode::Char('6'), event::KeyModifiers::NONE));
        assert_eq!(app.ui.current_card, initial_card);

        app.handle_key_event(KeyEvent::new(KeyCode::Char('x'), event::KeyModifiers::NONE));
        assert_eq!(app.ui.current_card, initial_card);
    }

    #[test]
    fn test_global_state_updates() {
        let mut app = create_test_app();

        // First refresh the global state
        refresh_global_state(&mut app.global_state);

        // Process a card with perfect quality
        app.update_state(Quality::Perfect);

        // Check global state updates
        assert!(app.global_state.last_revise_session.is_some());
        assert!(app.global_state.mean_q.is_some());
        assert_eq!(app.global_state.total_cards_revised, 1);
    }

    #[test]
    fn test_leech_card_handling() {
        let mut app = create_test_app();

        // Make the card a leech
        app.cards[0].leech = true;

        // Should still be able to review leech cards
        app.handle_key_event(KeyEvent::new(KeyCode::Char(' '), event::KeyModifiers::NONE));
        assert!(app.ui.revealed);

        app.handle_key_event(KeyEvent::new(KeyCode::Char('5'), event::KeyModifiers::NONE));
        assert!(!app.ui.revealed);
        assert_eq!(app.ui.current_card, 1);
    }

    #[test]
    fn test_orphan_card_handling() {
        let mut app = create_test_app();

        // Make the card an orphan
        app.cards[0].orphan = true;

        // Should still be able to review orphan cards
        app.handle_key_event(KeyEvent::new(KeyCode::Char(' '), event::KeyModifiers::NONE));
        assert!(app.ui.revealed);

        app.handle_key_event(KeyEvent::new(KeyCode::Char('5'), event::KeyModifiers::NONE));
        assert!(!app.ui.revealed);
    }

    #[test]
    fn test_revise_with_tagged_cards() {
        let mut app = create_test_app();
        app.cards[0].card.tags.insert("test_tag".to_string());

        // Cards with tags should be reviewable
        app.handle_key_event(KeyEvent::new(KeyCode::Char(' '), event::KeyModifiers::NONE));
        assert!(app.ui.revealed);

        app.handle_key_event(KeyEvent::new(KeyCode::Char('5'), event::KeyModifiers::NONE));
        assert_eq!(app.ui.current_card, 1);

        // One more update should trigger exit
        app.update_state(Quality::Perfect);
        assert!(app.ui.exit);
    }

    #[test]
    fn test_multiple_response_lines() {
        let mut app = create_test_app();
        app.cards[0].card.response = vec![
            "line 1".to_string(),
            "line 2".to_string(),
            "line 3".to_string(),
        ];

        // Should handle multi-line responses
        app.handle_key_event(KeyEvent::new(KeyCode::Char(' '), event::KeyModifiers::NONE));
        assert!(app.ui.revealed);
    }

    #[test]
    fn test_consecutive_failures() {
        let mut app = create_test_app();

        // Test that consecutive failures are tracked
        for _ in 0..2 {
            app.ui.current_card = 0; // Reset position
            app.update_state(Quality::IncorrectAndForgotten);
        }

        let card = &app.cards[0];
        assert_eq!(card.state.failed_count, 2);
        assert!(!card.leech); // Should not be leech yet

        // One more failure should make it a leech
        app.ui.current_card = 0;
        app.update_state(Quality::IncorrectAndForgotten);
        assert!(app.cards[0].leech);
    }

    #[test]
    fn test_state_persistence() {
        let mut app = create_test_app();

        // First refresh the global state
        refresh_global_state(&mut app.global_state);

        // Process a card
        app.update_state(Quality::Perfect);

        // Verify that the update_fn was called with correct state
        let card = &app.cards[0];
        assert_eq!(card.revise_count, 1);
        assert!(card.last_revised.is_some());

        // Global state should also be updated
        assert!(app.global_state.last_revise_session.is_some());
    }

    #[test]
    fn test_keyboard_shortcuts() {
        // Test various keyboard shortcuts
        let shortcuts = [
            (KeyCode::Char(' '), "reveal"),
            (KeyCode::Char('q'), "quit"),
            (KeyCode::Char('?'), "help"),
            // Add more shortcuts as needed
        ];

        for (key, action) in shortcuts.iter() {
            let mut app = create_test_app();
            app.handle_key_event(KeyEvent::new(*key, event::KeyModifiers::NONE));

            match *action {
                "reveal" => assert!(app.ui.revealed),
                "quit" => assert!(app.ui.exit),
                "help" => assert!(app.ui.help),
                _ => panic!("Unknown action"),
            }
        }
    }

    #[test]
    fn test_card_state_reset() {
        let mut app = create_test_app();

        // Process card with failure
        app.update_state(Quality::IncorrectAndForgotten);
        let card = &app.cards[0];
        assert_eq!(card.state.failed_count, 1);

        // Process same card with success
        app.ui.current_card = 0;
        app.update_state(Quality::Perfect);
        let card = &app.cards[0];
        assert_eq!(card.state.failed_count, 1); // Failed count should persist
        assert_eq!(card.revise_count, 2);
    }

    #[test]
    fn test_empty_response() {
        let mut app = create_test_app();
        app.cards[0].card.response = vec![];

        // Should handle empty response gracefully
        app.handle_key_event(KeyEvent::new(KeyCode::Char(' '), event::KeyModifiers::NONE));
        assert!(app.ui.revealed);
    }

    #[test]
    fn test_revise_with_multiple_tags() {
        let mut app = create_test_app();
        app.cards[0].card.tags.insert("tag1".to_string());
        app.cards[0].card.tags.insert("tag2".to_string());

        // Cards with multiple tags should be reviewable
        app.handle_key_event(KeyEvent::new(KeyCode::Char(' '), event::KeyModifiers::NONE));
        assert!(app.ui.revealed);
    }

    #[test]
    fn test_algorithm_updates() {
        let mut app = create_test_app();
        let initial_interval = app.cards[0].state.interval;

        // Perfect response should increase interval
        app.update_state(Quality::Perfect);
        assert!(app.cards[0].state.interval >= initial_interval);

        // Reset card state
        app = create_test_app();
        let initial_interval = app.cards[0].state.interval;

        // Failed response should reset or decrease interval
        app.update_state(Quality::IncorrectAndForgotten);
        assert!(app.cards[0].state.interval <= initial_interval);
    }

    #[test]
    fn test_review_timestamps() {
        let mut app = create_test_app();
        assert!(app.cards[0].last_revised.is_none());

        let before_review = chrono::Utc::now();
        app.update_state(Quality::Perfect);
        let after_review = chrono::Utc::now();

        let review_time = app.cards[0].last_revised.unwrap();
        assert!(review_time >= before_review && review_time <= after_review);
    }

    #[test]
    fn test_consecutive_perfect_scores() {
        let mut app = create_test_app();
        let initial_interval = app.cards[0].state.interval;

        // Multiple perfect scores should increase interval more
        for _ in 0..3 {
            app.ui.current_card = 0;
            app.update_state(Quality::Perfect);
        }

        assert!(app.cards[0].state.interval > initial_interval * 2);
    }

    #[test]
    fn test_keyboard_modifiers() {
        let mut app = create_test_app();

        // Modifiers should be ignored
        app.handle_key_event(KeyEvent::new(
            KeyCode::Char(' '),
            event::KeyModifiers::SHIFT,
        ));
        assert!(app.ui.revealed);

        app.handle_key_event(KeyEvent::new(
            KeyCode::Char('q'),
            event::KeyModifiers::CONTROL,
        ));
        assert!(app.ui.exit);
    }

    #[test]
    fn test_mean_quality_updates() {
        let mut app = create_test_app();
        refresh_global_state(&mut app.global_state);

        // Perfect score
        app.update_state(Quality::Perfect);
        let perfect_mean = app.global_state.mean_q.unwrap();

        // Reset and test with lower score
        let mut app = create_test_app();
        refresh_global_state(&mut app.global_state);
        app.update_state(Quality::IncorrectButRemembered);
        let lower_mean = app.global_state.mean_q.unwrap();

        assert!(perfect_mean > lower_mean);
    }

    #[test]
    fn test_card_progression_order() {
        let mut app = create_test_app();
        let second_card = app.cards[0].clone();
        app.cards.push(second_card);

        // Cards should be reviewed in order
        assert_eq!(app.ui.current_card, 0);
        app.update_state(Quality::Perfect);
        assert_eq!(app.ui.current_card, 1);
        app.update_state(Quality::Perfect);
        assert_eq!(app.ui.current_card, 2);
    }

    #[test]
    fn test_boundary_conditions() {
        let mut app = create_test_app();

        // Test with very large intervals
        app.cards[0].state.interval = u64::MAX / 2;
        app.update_state(Quality::Perfect);
        assert!(app.cards[0].state.interval < u64::MAX);

        app.ui.current_card = 0;

        // Test with very small intervals
        app.cards[0].state.interval = u64::MIN;
        app.update_state(Quality::IncorrectAndForgotten);
        assert_eq!(app.cards[0].state.interval, u64::MIN); // Should not go below minimum
    }

    #[test]
    fn test_help_screen_interaction() {
        let mut app = create_test_app();

        // Help screen should block card interactions
        app.handle_key_event(KeyEvent::new(KeyCode::Char('?'), event::KeyModifiers::NONE));
        assert!(app.ui.help);

        app.handle_key_event(KeyEvent::new(KeyCode::Char(' '), event::KeyModifiers::NONE));
        assert!(!app.ui.revealed); // Should not reveal card while help is shown

        app.handle_key_event(KeyEvent::new(KeyCode::Char('5'), event::KeyModifiers::NONE));
        assert_eq!(app.ui.current_card, 0); // Should not progress while help is shown

        // Close help and verify interactions work again
        app.handle_key_event(KeyEvent::new(KeyCode::Char('?'), event::KeyModifiers::NONE));
        assert!(!app.ui.help);

        app.handle_key_event(KeyEvent::new(KeyCode::Char(' '), event::KeyModifiers::NONE));
        assert!(app.ui.revealed);
    }
}
