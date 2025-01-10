mod simple8;
mod sm2;
mod sm5;

use clap::ValueEnum;
use ordered_float::OrderedFloat;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::db::GlobalState;

#[derive(Debug, Clone, ValueEnum)]
pub enum Algo {
    SM2,
    SM5,
    Simple8,
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

// repetitions -> ease_factor -> optimal_factor
pub type OptimalFactorMatrix = HashMap<u64, HashMap<OrderedFloat<f64>, f64>>;
// Clone for tests
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CardState {
    // The ease factor is used to determine the number of days to wait before reviewing again
    ease_factor: f64,
    // An integer number indicating the number of days to wait before the next review
    pub interval: u64,
    // The number of times the information has been reviewed prior to this review
    repetitions: u64,
    // The number of times the information has been reviewed and failed
    pub failed_count: u64,
}

impl Default for CardState {
    fn default() -> Self {
        Self {
            ease_factor: 2.5,
            interval: 0,
            repetitions: 0,
            failed_count: 0,
        }
    }
}

pub trait Algorithm {
    fn update_state(&self, quality: &Quality, state: &mut CardState, global: &mut GlobalState);
    fn name(&self) -> &'static str;
}

pub fn new_algorithm(algo: Algo) -> Box<dyn Algorithm> {
    match algo {
        Algo::SM2 => Box::new(sm2::Sm2 {}),
        Algo::SM5 => Box::new(sm5::Sm5 {}),
        Algo::Simple8 => Box::new(simple8::Simple8 {}),
    }
}

fn new_ease_factor(quality: &Quality, ease_factor: f64) -> f64 {
    let q = *quality as usize;
    let new_ef = ease_factor + 0.1 - (5.0 - q as f64) * (0.08 + (5.0 - q as f64) * 0.02);
    new_ef.max(1.3)
}

pub fn update_meanq(global: &mut GlobalState, quality: Quality) {
    let q = (quality as usize) as f64;
    let total = global.total_cards_revised as f64;
    global.total_cards_revised += 1;
    global.mean_q = Some(if let Some(mean_q) = global.mean_q {
        (total * mean_q + q) / (total + 1.0)
    } else {
        (quality as usize) as f64
    });
}

fn round_float(f: f64, fix: usize) -> f64 {
    let factor = 10.0_f64.powi(fix as i32);
    (f * factor).round() / factor
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_round_float() {
        assert_eq!(round_float(2.123456, 2), 2.12);
        assert_eq!(round_float(2.123456, 3), 2.123);
        assert_eq!(round_float(2.123456, 4), 2.1235);
    }

    #[test]
    fn test_update_meanq() {
        let mut global = GlobalState::default();
        update_meanq(&mut global, Quality::Perfect);
        assert_eq!(global.total_cards_revised, 1);
        assert_eq!(global.mean_q.unwrap(), 5.0);

        update_meanq(&mut global, Quality::CorrectWithHesitation);
        assert_eq!(global.total_cards_revised, 2);
        assert_eq!(global.mean_q.unwrap(), 4.5);

        update_meanq(&mut global, Quality::IncorrectAndForgotten);
        assert_eq!(global.total_cards_revised, 3);
        assert_eq!(global.mean_q.unwrap(), 3.0);
    }

    #[test]
    fn test_new_ease_factor() {
        let q = Quality::Perfect;
        let ef = 2.5;
        assert_eq!(new_ease_factor(&q, ef), 2.6);

        let q = Quality::IncorrectAndForgotten;
        let ef = 2.5;
        assert_eq!(round_float(new_ease_factor(&q, ef), 2), 1.70);
    }

    #[test]
    fn test_new_ease_factor_corner_cases() {
        // Test minimum boundary (1.3)
        let q = Quality::IncorrectAndForgotten;
        assert_eq!(new_ease_factor(&q, 1.2), 1.3);
        assert_eq!(new_ease_factor(&q, 1.0), 1.3);

        // Test all quality levels with a normal ease factor
        let ef = 2.5;
        assert_eq!(
            round_float(new_ease_factor(&Quality::IncorrectAndForgotten, ef), 2),
            1.70
        );
        assert_eq!(
            round_float(new_ease_factor(&Quality::IncorrectButRemembered, ef), 2),
            1.96
        );
        assert_eq!(
            round_float(new_ease_factor(&Quality::IncorrectButEasyToRecall, ef), 2),
            2.18
        );
        assert_eq!(
            round_float(new_ease_factor(&Quality::CorrectWithDifficulty, ef), 2),
            2.36
        );
        assert_eq!(
            round_float(new_ease_factor(&Quality::CorrectWithHesitation, ef), 2),
            2.50
        );
        assert_eq!(round_float(new_ease_factor(&Quality::Perfect, ef), 2), 2.60);
    }
}
