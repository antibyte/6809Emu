use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use m6809_core::{Emulator, StepResult};

/// CoCo/Dragon NTSC E-clock — used for real-time pacing when AY is off.
pub const DEFAULT_E_CLOCK_HZ: u32 = 894_886;

#[derive(Clone)]
pub struct RunSpeed {
    /// Emulation rate vs. real hardware (1.0 = one E-clock second per wall second).
    pub rate: f64,
    pub frame_ms: u64,
    /// Safety cap on instructions per UI frame (prevents infinite loops at Max).
    pub max_steps: u32,
}

impl Default for RunSpeed {
    fn default() -> Self {
        Self {
            rate: 1.0,
            frame_ms: 50,
            max_steps: 250_000,
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
