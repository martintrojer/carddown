pub mod sm2;
pub mod sm5;

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

// repetitions -> ease_factor -> optimal_factor
pub type OptimalFactorMatrix = HashMap<u64, HashMap<OrderedFloat<f64>, f64>>;

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
    fn update_state(&self, quality: &Quality, state: &mut CardState, global: &mut GlobalState);
    fn name(&self) -> &'static str;
}

pub fn new_algorithm(algo: Algo) -> Box<dyn Algorithm> {
    match algo {
        Algo::SM2 => Box::new(sm2::Sm2 {}),
        _ => Box::new(sm5::Sm5 {}),
    }
}

fn round_float(f: f64, fix: usize) -> f64 {
    let factor = 10.0_f64.powi(fix as i32);
    (f * factor).round() / factor
}

fn new_ease_factor(quality: &Quality, ease_factor: f64) -> f64 {
    if ease_factor < 1.3 {
        1.3
    } else {
        let q = *quality as usize;
        ease_factor + 0.1 - (5 - q) as f64 * (0.08 + (5 - q) as f64 * 0.02)
    }
}
