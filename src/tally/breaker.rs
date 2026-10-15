use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BreakerState {
    Closed,
    Open,
    HalfOpen,
}

pub struct CircuitBreaker {
    failure_threshold: u32,
    reset_timeout: Duration,
    consecutive_failures: AtomicU32,
    state: RwLock<BreakerState>,
    last_failure_time: RwLock<Option<Instant>>,
}

impl CircuitBreaker {
    pub fn new(failure_threshold: u32, reset_timeout_secs: u64) -> Self {
        Self {
            failure_threshold,
            reset_timeout: Duration::from_secs(reset_timeout_secs),
            consecutive_failures: AtomicU32::new(0),
            state: RwLock::new(BreakerState::Closed),
            last_failure_time: RwLock::new(None),
        }
    }

    pub async fn can_execute(&self) -> bool {
        let state = *self.state.read().await;
        match state {
            BreakerState::Closed => true,
            BreakerState::HalfOpen => true,
            BreakerState::Open => {
                let last_failure = self.last_failure_time.read().await;
                if let Some(time) = *last_failure {
                    if time.elapsed() >= self.reset_timeout {
                        drop(last_failure);
                        let mut state_lock = self.state.write().await;
                        *state_lock = BreakerState::HalfOpen;
                        true
                    } else {
                        false
                    }
                } else {
                    false
                }
            }
        }
    }

    pub async fn record_success(&self) {
        self.consecutive_failures.store(0, Ordering::Relaxed);
        let mut state_lock = self.state.write().await;
        *state_lock = BreakerState::Closed;
    }

    pub async fn record_failure(&self) {
        let failures = self.consecutive_failures.fetch_add(1, Ordering::Relaxed) + 1;
        let mut last_failure = self.last_failure_time.write().await;
        *last_failure = Some(Instant::now());

        if failures >= self.failure_threshold {
            let mut state_lock = self.state.write().await;
            *state_lock = BreakerState::Open;
        }
    }

    pub async fn state(&self) -> BreakerState {
        *self.state.read().await
    }

    #[allow(dead_code)]
    pub fn failure_count(&self) -> u32 {
        self.consecutive_failures.load(Ordering::Relaxed)
    }
}
