use std::time::Duration;


/// The schedule for backoff of the scheduler, after
/// these are exhausted we will fall back to eventfd.
const BACKOFF_SCHEDULE: &[(Duration, usize)] = &[
    (Duration::from_micros(10), 10),
    (Duration::from_micros(50), 10),
    (Duration::from_micros(150), 10),
    (Duration::from_micros(250), 25)
];

pub struct AdaptiveBackoff {
    backoff: (Duration, usize),
    spin_count: usize,
    idx: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackoffResult {
    Timeout(Duration),
    Park,
}

impl AdaptiveBackoff {
    pub fn new() -> Self {
        AdaptiveBackoff {
            backoff: BACKOFF_SCHEDULE[0],
            idx: 0,
            spin_count: 0,
        }
    }
    pub fn backoff(&mut self) -> BackoffResult {
        self.spin_count += 1;

        if self.idx == BACKOFF_SCHEDULE.len() {
            BackoffResult::Park
        } else {
            if self.backoff.1 == self.spin_count {
                self.spin_count = 0;

                self.idx += 1;
                if self.idx == BACKOFF_SCHEDULE.len() {
                    return BackoffResult::Park;
                } else {
                    self.backoff = BACKOFF_SCHEDULE[self.idx];
                }
            }
            BackoffResult::Timeout(self.backoff.0)
        }
    }
    pub fn reset(&mut self) {
        self.idx = 0;
        self.backoff = BACKOFF_SCHEDULE[0];
        self.spin_count = 0;
    }
}

#[cfg(test)]
mod tests {
    use crate::core::executor::backoff::{AdaptiveBackoff, BackoffResult, BACKOFF_SCHEDULE};

    #[test]
    pub fn test_adaptive_backoff() {
        let mut backoff = AdaptiveBackoff::new();
        for (time, reps) in BACKOFF_SCHEDULE {
            assert_eq!(backoff.backoff(), BackoffResult::Timeout(*time));
            // println!("TIME: {time:?}, REPS: {reps}");
            for _ in 0..(*reps - 1) {
                backoff.backoff();
            }
            // println!("Switch: {:?}", backoff.spin_count);
        }
        for _ in 0..10 {
            assert_eq!(backoff.backoff(), BackoffResult::Park);
        }

        // assert_eq!(backoff.backoff(), BackoffResult::Timeout(BACKOFF_SCHEDULE[0].0));
    }
}
