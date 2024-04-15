use crate::db::GlobalState;

use super::{Algorithm, CardState, Quality};

pub struct Sm2 {}

impl Algorithm for Sm2 {
    fn next_interval(&self, quality: &Quality, state: &mut CardState, _global: &mut GlobalState) {
        if !quality.failed() {
            match state.repetitions {
                0 => {
                    state.interval = 1;
                }
                1 => {
                    state.interval = 6;
                }
                _ => {
                    state.interval = (state.interval as f64 * state.ease_factor).round() as u64;
                }
            }

            state.repetitions += 1;
            let q = *quality as usize;
            state.ease_factor += 0.1 - (5 - q) as f64 * (0.08 + (5 - q) as f64 * 0.02);
        } else {
            state.repetitions = 0;
            state.interval = 0;
        }
        if state.ease_factor < 1.3 {
            state.ease_factor = 1.3;
        }
    }
}
