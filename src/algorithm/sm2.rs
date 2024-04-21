use crate::db::GlobalState;

use super::{new_ease_factor, Algorithm, CardState, Quality};

pub struct Sm2 {}

impl Algorithm for Sm2 {
    fn update_state(&self, quality: &Quality, state: &mut CardState, _global: &mut GlobalState) {
        if quality.failed() {
            state.repetitions = 0;
            state.interval = 0;
        } else {
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
            state.ease_factor = new_ease_factor(quality, state.ease_factor);
        }
    }
    fn name(&self) -> &'static str {
        "SM2"
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    use crate::{algorithm::round_float, db::GlobalState};

    #[test]
    fn test_sm2() {
        let mut state = CardState::default();
        let mut global = GlobalState::default();
        let sm2 = Sm2 {};

        sm2.update_state(&Quality::Perfect, &mut state, &mut global);
        assert_eq!(state.interval, 1);
        assert_eq!(state.repetitions, 1);
        assert_eq!(state.ease_factor, 2.6);

        sm2.update_state(&Quality::Perfect, &mut state, &mut global);
        assert_eq!(state.interval, 6);
        assert_eq!(state.repetitions, 2);
        assert_eq!(state.ease_factor, 2.7);

        sm2.update_state(&Quality::Perfect, &mut state, &mut global);
        assert_eq!(state.interval, 16);
        assert_eq!(state.repetitions, 3);
        assert_eq!(round_float(state.ease_factor, 2), 2.80);
        let prev_ef = state.ease_factor;

        sm2.update_state(&Quality::IncorrectAndForgotten, &mut state, &mut global);
        assert_eq!(state.interval, 0);
        assert_eq!(state.repetitions, 0);
        assert_eq!(state.ease_factor, prev_ef);
    }
}
