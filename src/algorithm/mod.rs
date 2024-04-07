mod sm2;

use serde::{Deserialize, Serialize};

// An integer from 0-5 indicating how easily the information was remembered today
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CardState {
    // The ease factor is used to determine the number of days to wait before reviewing again
    ease_factor: f64,
    // An integer number indicating the number of days to wait before the next review
    interval: u64,
    // The number of times the information has been reviewed prior to this review
    repetitions: u64,
}

impl CardState {
    pub fn new() -> Self {
        Self {
            ease_factor: 2.5,
            interval: 0,
            repetitions: 0,
        }
    }
}

trait Algorithm {
    fn next_interval(&self, quality: Quality, state: &CardState) -> CardState;
}
