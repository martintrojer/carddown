use super::{Algorithm, CardState, Quality};
use crate::db::GlobalState;

pub struct Simple8 {}

impl Algorithm for Simple8 {
    fn update_state(&self, quality: &Quality, state: &mut CardState, global: &mut GlobalState) {
        if quality.failed() {
            state.repetitions = 0;
            state.interval = 0;
        } else if state.repetitions == 0 || state.interval == 0 {
            state.interval = first_interval(state.failed_count) as u64;
            state.repetitions += 1;
        } else {
            let q = global.mean_q.unwrap_or((*quality as usize) as f64);
            let factor = interval_factor(quality_to_ease(q), state.repetitions);
            state.interval = (state.interval as f64 * factor).round() as u64;
            state.repetitions += 1;
        }
    }
    fn name(&self) -> &'static str {
        "Simple8"
    }
}

/// Returns optimal first interval for a card that has failed `total_failures` times.
fn first_interval(total_failures: u64) -> f64 {
    2.4849 * std::f64::consts::E.powf(-0.057 * total_failures as f64)
}

fn interval_factor(ease: f64, repetitions: u64) -> f64 {
    let r = repetitions as f64;
    1.2 + (ease - 1.2) * 0.5_f64.powf(r.log2())
}

fn quality_to_ease(q: f64) -> f64 {
    0.0542 * q.powi(4) + -0.4848 * q.powi(3) + 1.4916 * q.powi(2) + -1.2403 * q + 1.4515
}

#[cfg(test)]
mod tests {

    use crate::algorithm::update_meanq;

    use super::*;

    #[test]
    fn test_simple8() {
        let mut state = CardState::default();
        let mut global = GlobalState::default();
        let simple8 = Simple8 {};

        update_meanq(&mut global, Quality::Perfect);
        simple8.update_state(&Quality::Perfect, &mut state, &mut global);
        assert_eq!(state.interval, 2);
        assert_eq!(state.repetitions, 1);

        update_meanq(&mut global, Quality::Perfect);
        simple8.update_state(&Quality::Perfect, &mut state, &mut global);
        assert_eq!(state.interval, 12);
        assert_eq!(state.repetitions, 2);

        update_meanq(&mut global, Quality::Perfect);
        simple8.update_state(&Quality::Perfect, &mut state, &mut global);
        assert_eq!(state.interval, 42);
        assert_eq!(state.repetitions, 3);

        update_meanq(&mut global, Quality::IncorrectAndForgotten);
        simple8.update_state(&Quality::IncorrectAndForgotten, &mut state, &mut global);
        assert_eq!(state.interval, 0);
        assert_eq!(state.repetitions, 0);
    }
}
