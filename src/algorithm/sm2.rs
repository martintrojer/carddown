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

    #[test]
    fn test_sm2_edge_cases() {
        let mut state = CardState::default();
        let mut global = GlobalState::default();
        let sm2 = Sm2 {};

        // Test consecutive failures
        sm2.update_state(&Quality::IncorrectAndForgotten, &mut state, &mut global);
        assert_eq!(state.interval, 0);
        assert_eq!(state.repetitions, 0);
        assert_eq!(state.ease_factor, 2.5); // Default ease factor

        sm2.update_state(&Quality::IncorrectButRemembered, &mut state, &mut global);
        assert_eq!(state.interval, 0);
        assert_eq!(state.repetitions, 0);
        assert_eq!(state.ease_factor, 2.5); // Should remain unchanged

        // Test recovery after failure
        sm2.update_state(&Quality::Perfect, &mut state, &mut global);
        assert_eq!(state.interval, 1);
        assert_eq!(state.repetitions, 1);
        assert_eq!(state.ease_factor, 2.6);

        // Test minimum ease factor boundary
        state.ease_factor = 1.3; // Set to minimum
        sm2.update_state(&Quality::CorrectWithDifficulty, &mut state, &mut global);
        assert_eq!(state.interval, 6);
        assert_eq!(state.repetitions, 2);
        assert_eq!(round_float(state.ease_factor, 2), 1.3); // Should not go below 1.3

        // Test very large intervals
        state.interval = 1000;
        state.ease_factor = 2.5;
        sm2.update_state(&Quality::Perfect, &mut state, &mut global);
        assert_eq!(state.interval, 2500);
        assert_eq!(state.repetitions, 3);

        // Test all quality levels
        state = CardState::default(); // Reset state
        for quality in [
            Quality::IncorrectAndForgotten,
            Quality::IncorrectButRemembered,
            Quality::IncorrectButEasyToRecall,
            Quality::CorrectWithDifficulty,
            Quality::CorrectWithHesitation,
            Quality::Perfect,
        ] {
            sm2.update_state(&quality, &mut state, &mut global);
            if quality.failed() {
                assert_eq!(state.interval, 0);
                assert_eq!(state.repetitions, 0);
            } else {
                assert!(state.interval > 0);
                assert!(state.repetitions > 0);
            }
        }
    }
}
