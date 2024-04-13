use super::{Algorithm, CardState, Quality};

pub struct Sm2 {}

impl Algorithm for Sm2 {
    fn next_interval(&self, quality: &Quality, state: &CardState) -> CardState {
        let mut res = state.clone();
        let q = *quality as usize;

        if q >= 3 {
            match state.interval {
                0 => {
                    res.interval = 1;
                }
                1 => {
                    res.interval = 6;
                }
                _ => {
                    res.interval = (state.interval as f64 * state.ease_factor).round() as u64;
                }
            }

            res.repetitions += 1;
            res.ease_factor =
                state.ease_factor + (0.1 - (5 - q) as f64 * (0.08 + (5 - q) as f64 * 0.02));
        } else {
            res.repetitions = 0;
            res.interval = 1;
            res.ease_factor = state.ease_factor;
        }
        if res.ease_factor < 1.3 {
            res.ease_factor = 1.3;
        }
        res
    }
}
