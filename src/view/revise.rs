use crate::algorithm::{update_meanq, Algorithm, Quality};
use anyhow::Result;
use chrono::{DateTime, Local};
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

pub struct App {
    algorithm: Box<dyn Algorithm>,
    cards: Vec<CardEntry>,
    global_state: GlobalState,
    leech_threshold: usize,
    max_duration: usize,
    reverse_probability: f64,
    tags: Vec<String>,
    #[allow(clippy::type_complexity)]
    update_fn: Box<dyn Fn(Vec<CardEntry>, &GlobalState) -> Result<()>>,
    ui: UiState,
}

impl App {
    #[allow(clippy::type_complexity, clippy::too_many_arguments)]
    pub fn new(
        algorithm: Box<dyn Algorithm>,
        cards: Vec<CardEntry>,
        global_state: GlobalState,
        leech_threshold: usize,
        max_duration: usize,
        reverse_probability: f64,
        tags: Vec<String>,
        update_fn: Box<dyn Fn(Vec<CardEntry>, &GlobalState) -> Result<()>>,
    ) -> Self {
        Self {
            algorithm,
            cards,
            update_fn,
            global_state,
            leech_threshold,
            max_duration,
            reverse_probability,
            tags,
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
        frame.render_widget(self, frame.size());
    }

    /// updates the application's state based on user input
    fn handle_events(&mut self) -> io::Result<()> {
        if let Ok(true) = event::poll(Duration::from_secs(1)) {
            if self.ui.started.elapsed().as_secs() >= self.max_duration as u64 {
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
        let current_card = self.ui.current_card;
        if self.cards.is_empty() {
            return;
        }
        if self.ui.current_card >= self.cards.len() {
            self.exit();
        } else {
            self.ui.current_card += 1;
        }
        update_meanq(&mut self.global_state, quality);
        if let Some(card) = self.cards.get_mut(current_card) {
            card.last_revised = Some(chrono::Utc::now());
            card.revise_count += 1;
            self.algorithm
                .update_state(&quality, &mut card.state, &mut self.global_state);
            if quality.failed() {
                card.state.failed_count += 1;
            }
            if card.state.failed_count >= self.leech_threshold as u64 {
                card.leech = true;
            }
        }
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

    fn exit(&mut self) {
        (self.update_fn)(std::mem::take(&mut self.cards), &self.global_state).unwrap();
        self.ui.exit = true;
    }

    fn help(&self) -> (Block, Text) {
        let title = Title::from(" Key Bindings ".bold());
        let secs = self.ui.started.elapsed().as_secs();
        let min = secs / 60;
        let secs = secs % 60;
        let instructions = Title::from(Line::from(vec![
            " Quit ".into(),
            "<Q> ".bold(),
            "Elapsed ".into(),
            format!("{}:{:02} ", min, secs).bold(),
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

    fn card_revise(&self) -> (Block, Text) {
        let title = Title::from(
            format!(
                " Revise Cards {}/{} [{}] ",
                std::cmp::min(self.cards.len(), 1 + self.ui.current_card),
                self.cards.len(),
                if self.tags.is_empty() {
                    "All Tags".to_string()
                } else {
                    self.tags.join(", ")
                }
            )
            .bold(),
        );
        let secs = self.ui.started.elapsed().as_secs();
        let min = secs / 60;
        let secs = secs % 60;
        let instructions = Title::from(Line::from(vec![
            " Quit ".into(),
            "<Q> ".bold(),
            "Reveal ".into(),
            "<Space> ".blue().bold(),
            "Score/Quality ".into(),
            "<0-5> ".green().bold(),
            "Help ".into(),
            "<?> ".bold(),
            "Elapsed ".into(),
            format!("{}:{:02} ", min, secs).bold(),
            format!(" [{}] ", self.algorithm.name()).bold(),
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
        let reversed =
            self.reverse_probability > 0.0 && rand::random::<f64>() < self.reverse_probability;
        let counter_text = if self.cards.is_empty() || self.ui.current_card >= self.cards.len() {
            Text::from(vec![Line::from(vec!["No cards to revise".into()])])
        } else {
            let card = self.cards.get(self.ui.current_card).unwrap();
            let mut lines: Vec<Line> = Vec::new();
            lines.push(if card.leech {
                Line::from(vec!["Leech Card".red().bold()])
            } else if card.orphan {
                Line::from(vec!["Orphan Card".yellow().bold()])
            } else {
                Line::from(vec!["".into()])
            });
            lines.push(Line::from(vec![]));
            lines.push(Line::from(vec!["Prompt".bold()]));
            if reversed && !self.ui.revealed {
                lines.push(Line::from(vec!["<hidden>".into()]));
            } else {
                lines.push(Line::from(vec![card.card.prompt.clone().into()]));
            }
            lines.push(Line::from(vec![]));
            lines.push(Line::from(vec!["Response".bold()]));
            if reversed || self.ui.revealed {
                for l in card.card.response.lines() {
                    lines.push(Line::from(vec![l.into()]));
                }
            } else {
                lines.push(Line::from(vec!["<hidden>".into()]));
            }
            lines.push(Line::from(vec![]));
            lines.push(Line::from(vec!["Last Revised".bold()]));
            lines.push(Line::from(vec![card
                .last_revised
                .map(|d| {
                    let l: DateTime<Local> = DateTime::from(d);
                    l.format("%Y-%m-%d %H:%M").to_string()
                })
                .unwrap_or("Never".to_string())
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
            .centered()
            .block(block)
            .render(area, buf);
    }
}
