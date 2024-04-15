pub mod sm2;

use clap::ValueEnum;
use serde::{Deserialize, Serialize};

use crate::db::GlobalState;

#[derive(Debug, Clone, ValueEnum)]
pub enum Algo {
    SM2,
    SM5,
    Simple8,
    Leitner,
}

// An integer from 0-5 indicating how easily the information was remembered today
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum Quality {
    Perfect = 5,
    CorrectWithHesitation = 4,
    CorrectWithDifficulty = 3,
    IncorrectButEasyToRecall = 2,
    IncorrectButRemembered = 1,
    IncorrectAndForgotten = 0,
}

impl Quality {
    pub fn failed(&self) -> bool {
        matches!(
            self,
            Self::IncorrectAndForgotten
                | Self::IncorrectButRemembered
                | Self::IncorrectButEasyToRecall
        )
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct CardState {
    // The ease factor is used to determine the number of days to wait before reviewing again
    ease_factor: f64,
    // An integer number indicating the number of days to wait before the next review
    pub interval: u64,
    // The number of times the information has been reviewed prior to this review
    repetitions: u64,
}

impl Default for CardState {
    fn default() -> Self {
        Self {
            ease_factor: 2.5,
            interval: 0,
            repetitions: 0,
        }
    }
}

pub trait Algorithm {
    fn next_interval(&self, quality: &Quality, state: &mut CardState, global: &mut GlobalState);
}
