use anyhow::Result;
use crossterm::{execute, terminal::*};
use ratatui::prelude::*;
use std::io;
use std::io::{stdout, Stdout};
use std::time::{Duration, Instant};

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
    started: Instant,
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
            started: Instant::now(),
            sure: false,
        }
    }

    /// runs the application's main loop until the user quits
    pub fn run(&mut self, terminal: &mut Tui) -> io::Result<()> {
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
                self.sure = true;
            }
            KeyCode::Char('n') | KeyCode::Char('N') => {
                self.sure = false;
            }
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                if self.sure {
                    let card = self.cards.remove(self.current_card);
                    (self.delete_fn)(card.card.id).unwrap();
                    self.current_card = std::cmp::min(self.cards.len(), self.current_card);
                    self.sure = false;
                }
            }
            KeyCode::Left => {
                if self.cards.is_empty() {
                    return;
                }
                if self.current_card > 0 {
                    self.current_card -= 1;
                }
            }
            KeyCode::Right => {
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
            "<Q> ".blue().bold(),
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
        let title = Title::from(" Audit Cards ".bold());
        let secs = self.started.elapsed().as_secs();
        let min = (secs / 60) as u64;
        let secs = secs % 60;
        let instructions = Title::from(Line::from(vec![
            " Quit ".into(),
            "<Q> ".blue().bold(),
            "<Left>/<Right>".blue().bold(),
            " Delete ".into(),
            "<D> ".red().bold(),
            "[".into(),
            std::cmp::min(self.cards.len(), 1 + self.current_card)
                .to_string()
                .into(),
            "/".into(),
            self.cards.len().to_string().into(),
            "] ".into(),
            " Elapsed ".into(),
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

        let counter_text = if self.cards.is_empty() || self.current_card >= self.cards.len() {
            Text::from(vec![Line::from(vec!["No cards to audit".into()])])
        } else {
            let card = self.cards.get(self.current_card).unwrap();
            Text::from(vec![
                if card.orphan {
                    Line::from(vec!["Orphan".yellow().bold()])
                } else if card.leech {
                    Line::from(vec!["Leech".yellow().bold()])
                } else {
                    Line::from(vec!["".into()])
                },
                Line::from(vec![]),
                Line::from(vec!["Prompt".bold()]),
                Line::from(vec![card.card.prompt.clone().into()]),
                Line::from(vec![]),
                Line::from(vec!["Response".bold()]),
                Line::from(vec![card.card.response.clone().into()]),
                Line::from(vec![]),
                Line::from(vec!["Last Reviewed".bold()]),
                Line::from(vec![card
                    .last_reviewed
                    .map(|d| d.to_string())
                    .unwrap_or("Never".to_string())
                    .into()]),
                Line::from(vec![]),
                Line::from(vec!["File".bold()]),
                Line::from(vec![
                    card.card.file.to_string_lossy().into(),
                    ":".into(),
                    card.card.line.to_string().into(),
                ]),
            ])
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
            .centered()
            .block(block)
            .render(area, buf);
    }
}

/// A type alias for the terminal type used in this application
pub type Tui = Terminal<CrosstermBackend<Stdout>>;

/// Initialize the terminal
pub fn init() -> io::Result<Tui> {
    execute!(stdout(), EnterAlternateScreen)?;
    enable_raw_mode()?;
    Terminal::new(CrosstermBackend::new(stdout()))
}

/// Restore the terminal to its original state
pub fn restore() -> io::Result<()> {
    execute!(stdout(), LeaveAlternateScreen)?;
    disable_raw_mode()?;
    Ok(())
}
