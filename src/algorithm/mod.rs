mod sm2;

use serde::{Deserialize, Serialize};

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
    ease_factor: f64,
    interval: u64,
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
