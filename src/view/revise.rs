use crate::algorithm::sm2::Sm2;
use crate::algorithm::{Algo, Algorithm, Quality};
use anyhow::Result;
use chrono::{DateTime, Local};
use ratatui::prelude::*;
use std::io;
use std::time::{Duration, Instant};

use crate::db::CardEntry;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind};
use ratatui::{
    symbols::border,
    widgets::{block::*, *},
};

pub struct App {
    algorithm: Algo,
    cards: Vec<CardEntry>,
    current_card: usize,
    exit: bool,
    help: bool,
    leech_threshold: usize,
    max_duration: usize,
    revealed: bool,
    started: Instant,
    update_fn: Box<dyn Fn(Vec<CardEntry>) -> Result<()>>,
}

impl App {
    pub fn new(
        cards: Vec<CardEntry>,
        algorithm: Algo,
        max_duration: usize,
        leech_threshold: usize,
        update_fn: Box<dyn Fn(Vec<CardEntry>) -> Result<()>>,
    ) -> Self {
        Self {
            algorithm,
            cards,
            update_fn,
            current_card: 0,
            exit: false,
            help: false,
            leech_threshold,
            max_duration,
            revealed: false,
            started: Instant::now(),
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
            if self.started.elapsed().as_secs() >= self.max_duration as u64 {
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

    fn update_card_state(&mut self, quality: Quality) {
        self.revealed = false;
        if self.current_card < (self.cards.len() - 1) {
            self.current_card += 1;
        }
        if let Some(card) = self.cards.get_mut(self.current_card) {
            card.last_reviewed = Some(chrono::Utc::now());
            let algorithm = match self.algorithm {
                _ => Sm2 {},
            };
            card.state = algorithm.next_interval(&quality, &card.state);
            if quality.failed() {
                card.failed_count += 1;
            }
            if card.failed_count >= self.leech_threshold as u64 {
                card.leech = true;
            }
        }
    }

    fn handle_key_event(&mut self, key_event: KeyEvent) {
        match key_event.code {
            KeyCode::Char('q') | KeyCode::Char('Q') => self.exit(),
            KeyCode::Char(' ') => self.revealed = true,
            KeyCode::Char('?') => self.help = !self.help,
            KeyCode::Char('0') | KeyCode::Char('a') => {
                self.update_card_state(Quality::IncorrectAndForgotten)
            }
            KeyCode::Char('1') | KeyCode::Char('d') => {
                self.update_card_state(Quality::IncorrectButRemembered)
            }
            KeyCode::Char('2') | KeyCode::Char('g') => {
                self.update_card_state(Quality::IncorrectButEasyToRecall)
            }
            KeyCode::Char('3') | KeyCode::Char('j') => {
                self.update_card_state(Quality::CorrectWithDifficulty)
            }
            KeyCode::Char('4') | KeyCode::Char('l') => {
                self.update_card_state(Quality::CorrectWithHesitation)
            }
            KeyCode::Char('5') | KeyCode::Char('\'') => self.update_card_state(Quality::Perfect),
            _ => {}
        }
    }

    fn exit(&mut self) {
        (self.update_fn)(std::mem::take(&mut self.cards)).unwrap();
        self.exit = true;
    }

    fn card_revise(&self) -> (Block, Text) {
        let title = Title::from(
            format!(
                " Revise Cards {}/{}",
                std::cmp::min(self.cards.len(), 1 + self.current_card),
                self.cards.len()
            )
            .bold(),
        );
        let secs = self.started.elapsed().as_secs();
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

        let counter_text = if self.help {
            Text::from(vec![
                Line::from(vec![]),
                Line::from(vec!["Qualities".into()]),
                Line::from(vec![]),
                Line::from(vec![format!(
                    "{}/{}: {:?}",
                    Quality::IncorrectAndForgotten as u8,
                    'a',
                    Quality::IncorrectAndForgotten
                )
                .red()]),
                Line::from(vec![format!(
                    "{}/{}: {:?}",
                    Quality::IncorrectButRemembered as u8,
                    'd',
                    Quality::IncorrectButRemembered
                )
                .red()]),
                Line::from(vec![format!(
                    "{}/{}: {:?}",
                    Quality::IncorrectButEasyToRecall as u8,
                    'g',
                    Quality::IncorrectButEasyToRecall
                )
                .red()]),
                Line::from(vec![format!(
                    "{}/{}: {:?}",
                    Quality::CorrectWithDifficulty as u8,
                    'j',
                    Quality::CorrectWithDifficulty
                )
                .yellow()]),
                Line::from(vec![format!(
                    "{}/{}: {:?}",
                    Quality::CorrectWithHesitation as u8,
                    'l',
                    Quality::CorrectWithHesitation
                )
                .yellow()]),
                Line::from(vec![format!(
                    "{}/{}: {:?}",
                    Quality::Perfect as u8,
                    '\'',
                    Quality::Perfect
                )
                .green()]),
            ])
        } else {
            let card = self.cards.get(self.current_card).unwrap();
            let mut lines: Vec<Line> = Vec::new();
            lines.push(if card.orphan {
                Line::from(vec!["Orphan".yellow().bold()])
            } else if card.leech {
                Line::from(vec!["Leech".red().bold()])
            } else {
                Line::from(vec!["".into()])
            });
            lines.push(Line::from(vec![]));
            lines.push(Line::from(vec!["Prompt".bold()]));
            lines.push(Line::from(vec![card.card.prompt.clone().into()]));
            lines.push(Line::from(vec![]));
            lines.push(Line::from(vec!["Response".bold()]));
            if self.revealed {
                for l in card.card.response.lines() {
                    lines.push(Line::from(vec![l.into()]));
                }
            } else {
                lines.push(Line::from(vec!["<hidden>".into()]));
            }
            lines.push(Line::from(vec![]));
            lines.push(Line::from(vec!["Last Reviewed".bold()]));
            lines.push(Line::from(vec![card
                .last_reviewed
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
        let (block, counter_text) = self.card_revise();
        Paragraph::new(counter_text)
            .centered()
            .block(block)
            .render(area, buf);
    }
}
