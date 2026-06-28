use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use m6809_core::{Emulator, StepResult};

#[derive(Clone)]
pub struct RunSpeed {
    pub steps_per_tick: u32,
    pub frame_ms: u64,
}

impl Default for RunSpeed {
    fn default() -> Self {
        Self {
            steps_per_tick: 500,
            frame_ms: 50,
        }
    }
}

pub struct AppState {
    pub emulator: Mutex<Emulator>,
    pub running: Arc<AtomicBool>,
    pub trace: Mutex<Vec<StepResult>>,
    pub run_speed: Mutex<RunSpeed>,
    pub trace_limit: Mutex<usize>,
}

impl AppState {
    pub fn push_trace(&self, step: StepResult) {
        if let Ok(mut trace) = self.trace.lock() {
            let limit = self
                .trace_limit
                .lock()
                .map(|l| *l)
                .unwrap_or(200);
            trace.push(step);
            if trace.len() > limit {
                let drain = trace.len() - limit;
                trace.drain(0..drain);
            }
        }
    }
}