use anyhow::Result;
use chrono::{DateTime, Local};
use ratatui::prelude::*;
use std::io;
use std::time::Duration;

use crate::db::CardEntry;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind};
use ratatui::{
    symbols::border,
    widgets::{block::*, *},
};

pub struct App {
    cards: Vec<CardEntry>,
    current_card: usize,
    exit: bool,
    sure: bool,
    delete_fn: Box<dyn Fn(blake3::Hash) -> Result<()>>,
}

impl App {
    pub fn new(cards: Vec<CardEntry>, delete_fn: Box<dyn Fn(blake3::Hash) -> Result<()>>) -> Self {
        Self {
            cards,
            delete_fn,
            current_card: 0,
            exit: false,
            sure: false,
        }
    }

    /// runs the application's main loop until the user quits
    pub fn run(&mut self, terminal: &mut super::Tui) -> io::Result<()> {
        while !self.exit {
            terminal.draw(|frame| self.render_frame(frame))?;
            self.handle_events()?;
        }
        Ok(())
    }

    fn render_frame(&self, frame: &mut Frame) {
        frame.render_widget(self, frame.size());
    }

    /// updates the application's state based on user input
    fn handle_events(&mut self) -> io::Result<()> {
        if let Ok(true) = event::poll(Duration::from_secs(1)) {
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

    fn handle_key_event(&mut self, key_event: KeyEvent) {
        match key_event.code {
            KeyCode::Char('q') | KeyCode::Char('Q') => self.exit(),
            KeyCode::Char('d') | KeyCode::Char('D') => {
                if !self.cards.is_empty() {
                    self.sure = true;
                }
            }
            KeyCode::Char('n') | KeyCode::Char('N') => {
                self.sure = false;
            }
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                if self.sure {
                    let card = self.cards.remove(self.current_card);
                    if card.leech {
                        // Make no sense to delete a leech card
                        self.cards.push(card);
                    } else {
                        if let Err(_) = (self.delete_fn)(card.card.id) {
                            // If deletion fails, put the card back
                            self.cards.insert(self.current_card, card);
                        } else {
                            // After successful deletion, ensure current_card is valid
                            if self.current_card >= self.cards.len() && !self.cards.is_empty() {
                                self.current_card = self.cards.len() - 1;
                            }
                        }
                        self.sure = false;
                    }
                }
            }
            KeyCode::Left | KeyCode::Char('h') | KeyCode::Char('k') => {
                if self.cards.is_empty() {
                    return;
                }
                if self.current_card > 0 {
                    self.current_card -= 1;
                }
            }
            KeyCode::Right | KeyCode::Char('l') | KeyCode::Char('j') => {
                if self.cards.is_empty() {
                    return;
                }
                if self.current_card < (self.cards.len() - 1) {
                    self.current_card += 1;
                }
            }
            _ => {}
        }
    }

    fn exit(&mut self) {
        self.exit = true;
    }

    fn are_you_sure(&self) -> (Block, Text) {
        let title = Title::from(" Are You Sure? ".bold());
        let instructions = Title::from(Line::from(vec![
            " Quit ".into(),
            "<Q> ".bold(),
            " Yes ".into(),
            "<Y> ".blue().bold(),
            " No ".into(),
            "<N> ".red().bold(),
        ]));
        let block = Block::default()
            .title(title.alignment(Alignment::Center))
            .title(
                instructions
                    .alignment(Alignment::Center)
                    .position(Position::Bottom),
            )
            .borders(Borders::ALL)
            .border_set(border::DOUBLE);
        let counter_text = Text::from(vec![
            Line::from(vec![]),
            Line::from(vec![]),
            Line::from(vec![]),
            Line::from(vec![]),
            Line::from(vec!["Are You Sure?".red().bold()]),
        ]);
        (block, counter_text)
    }

    fn card_audit(&self) -> (Block, Text) {
        let title = Title::from(
            format!(
                " Audit Cards {}/{}",
                std::cmp::min(self.cards.len(), 1 + self.current_card),
                self.cards.len()
            )
            .bold(),
        );
        let instructions = Title::from(Line::from(vec![
            " Quit ".into(),
            "<Q> ".blue().bold(),
            "<Left> <Right>".green().bold(),
            " Delete ".into(),
            "<D> ".red().bold(),
        ]));
        let block = Block::default()
            .title(title.alignment(Alignment::Center))
            .title(
                instructions
                    .alignment(Alignment::Center)
                    .position(Position::Bottom),
            )
            .borders(Borders::ALL)
            .border_set(border::ROUNDED);

        let counter_text = if self.cards.is_empty() || self.current_card >= self.cards.len() {
            Text::from(vec![Line::from(vec!["No cards to audit".into()])])
        } else if let Some(card) = self.cards.get(self.current_card) {
            let mut lines: Vec<Line> = vec![];
            lines.push(if card.orphan {
                Line::from(vec!["Orphan".yellow().bold()])
            } else if card.leech {
                Line::from(vec!["Leech".yellow().bold()])
            } else {
                Line::from(vec!["".into()])
            });
            lines.push(Line::from(vec![]));
            lines.push(Line::from(vec!["Prompt".bold()]));
            lines.push(Line::from(vec![card.card.prompt.clone().into()]));
            lines.push(Line::from(vec![]));
            lines.push(Line::from(vec!["Response".bold()]));
            for l in card.card.response.iter() {
                lines.push(Line::from(vec![l.into()]));
            }
            lines.push(Line::from(vec![]));
            lines.push(Line::from(vec!["Tags".bold()]));
            lines.push(Line::from(vec![card
                .card
                .tags
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join(", ")
                .into()]));
            lines.push(Line::from(vec![]));
            lines.push(Line::from(vec!["Revise Count".bold()]));
            lines.push(Line::from(vec![card.revise_count.to_string().into()]));
            lines.push(Line::from(vec![]));
            lines.push(Line::from(vec!["Last Revised".bold()]));
            lines.push(Line::from(vec![card
                .last_revised
                .map(|d| {
                    let l: DateTime<Local> = DateTime::from(d);
                    l.format("%Y-%m-%d %H:%M").to_string()
                })
                .unwrap_or("never".to_string())
                .into()]));
            lines.push(Line::from(vec![]));
            lines.push(Line::from(vec!["Added".bold()]));
            lines.push(Line::from(vec![{
                let l: DateTime<Local> = DateTime::from(card.added);
                l.format("%Y-%m-%d %H:%M").to_string().into()
            }]));
            lines.push(Line::from(vec![]));
            lines.push(Line::from(vec!["File".bold()]));
            lines.push(Line::from(vec![
                card.card.file.to_string_lossy().into(),
                ":".into(),
                card.card.line.to_string().into(),
            ]));
            Text::from(lines)
        } else {
            Text::from(vec![Line::from(vec!["No cards to audit".into()])])
        };

        (block, counter_text)
    }
}

impl Widget for &App {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let (block, counter_text) = if self.sure {
            self.are_you_sure()
        } else {
            self.card_audit()
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
    use crate::algorithm::CardState;
    use chrono::Utc;
    use std::collections::HashSet;
    use std::path::PathBuf;

    fn create_test_card() -> CardEntry {
        CardEntry {
            card: crate::card::Card {
                id: blake3::hash(b"test"),
                prompt: "test prompt".to_string(),
                response: vec!["test response".to_string()],
                tags: HashSet::from_iter(vec!["test_tag".to_string()]),
                file: PathBuf::from("test/file.md"),
                line: 1,
            },
            revise_count: 0,
            last_revised: None,
            added: Utc::now(),
            orphan: false,
            leech: false,
            state: CardState::default(),
        }
    }

    #[test]
    fn test_navigation() {
        let cards = vec![create_test_card(), create_test_card(), create_test_card()];
        let delete_fn: Box<dyn Fn(blake3::Hash) -> Result<()>> = Box::new(|_| Ok(()));
        let mut app = App::new(cards, delete_fn);

        // Test initial state
        assert_eq!(app.current_card, 0);

        // Test right navigation
        app.handle_key_event(KeyEvent::new(KeyCode::Right, event::KeyModifiers::empty()));
        assert_eq!(app.current_card, 1);

        // Test left navigation
        app.handle_key_event(KeyEvent::new(KeyCode::Left, event::KeyModifiers::empty()));
        assert_eq!(app.current_card, 0);

        // Test bounds
        app.handle_key_event(KeyEvent::new(KeyCode::Left, event::KeyModifiers::empty()));
        assert_eq!(app.current_card, 0); // Should not go below 0
    }

    #[test]
    fn test_delete_flow() {
        let cards = vec![create_test_card()];
        let delete_fn: Box<dyn Fn(blake3::Hash) -> Result<()>> = Box::new(|_| Ok(()));
        let mut app = App::new(cards, delete_fn);

        // Test delete initiation
        app.handle_key_event(KeyEvent::new(
            KeyCode::Char('d'),
            event::KeyModifiers::empty(),
        ));
        assert!(app.sure);

        // Test cancel delete
        app.handle_key_event(KeyEvent::new(
            KeyCode::Char('n'),
            event::KeyModifiers::empty(),
        ));
        assert!(!app.sure);
        assert_eq!(app.cards.len(), 1);

        // Test confirm delete
        app.handle_key_event(KeyEvent::new(
            KeyCode::Char('d'),
            event::KeyModifiers::empty(),
        ));
        app.handle_key_event(KeyEvent::new(
            KeyCode::Char('y'),
            event::KeyModifiers::empty(),
        ));
        assert!(!app.sure);
        assert_eq!(app.cards.len(), 0);
    }

    #[test]
    fn test_leech_card_deletion() {
        let mut card = create_test_card();
        card.leech = true;
        let cards = vec![card];
        let delete_fn: Box<dyn Fn(blake3::Hash) -> Result<()>> = Box::new(|_| Ok(()));
        let mut app = App::new(cards, delete_fn);

        // Attempt to delete leech card
        app.handle_key_event(KeyEvent::new(
            KeyCode::Char('d'),
            event::KeyModifiers::empty(),
        ));
        app.handle_key_event(KeyEvent::new(
            KeyCode::Char('y'),
            event::KeyModifiers::empty(),
        ));

        // Verify leech card wasn't deleted
        assert_eq!(app.cards.len(), 1);
        assert!(app.cards[0].leech);
    }
}
