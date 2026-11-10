//! Bounded access-unit handoff with explicit reference-frame invalidation.

use std::collections::VecDeque;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Condvar, Mutex, MutexGuard,
};
use std::time::Duration;

const MAX_UNITS: usize = 8;
const MAX_UNIT_BYTES: usize = 1024 * 1024;

#[derive(Default)]
struct QueueState {
    units: VecDeque<Vec<u8>>,
    reset: bool,
}

/// A bounded single-decoder input queue that invalidates synchronization on loss.
#[derive(Default)]
pub struct DecodeQueue {
    /// True only after a decoder has produced a frame since the latest reset.
    pub synced: Arc<AtomicBool>,
    // A short lock serializes producers with reset consumption; native decoding stays outside it.
    state: Mutex<QueueState>,
    ready: Condvar,
}

impl DecodeQueue {
    /// Enqueues a unit, returning false if loss requires decoder reset and PLI.
    pub fn push(&self, au: Vec<u8>) -> bool {
        let mut state = self.lock();
        let fits = !au.is_empty() && au.len() <= MAX_UNIT_BYTES;
        let accepted = fits && state.units.len() < MAX_UNITS;
        if !accepted {
            state.units.clear();
            state.reset = true;
            self.synced.store(false, Ordering::Release);
        }
        if fits {
            state.units.push_back(au);
        }
        drop(state);
        self.ready.notify_one();
        accepted
    }
    /// Discards queued units and wakes the decoder to invalidate its references.
    pub fn reset(&self) {
        let mut state = self.lock();
        state.units.clear();
        state.reset = true;
        self.synced.store(false, Ordering::Release);
        drop(state);
        self.ready.notify_one();
    }

    /// Marks a decoded frame synchronized only if no reset arrived during decoding.
    pub fn finish_frame(&self) -> bool {
        let state = self.lock();
        if state.reset {
            return false;
        }
        self.synced.store(true, Ordering::Release);
        true
    }
    /// Waits at most `timeout`, returning a pending reset before any next unit.
    pub fn pop(&self, timeout: Duration) -> (bool, Option<Vec<u8>>) {
        let state = self.lock();
        // The owner polls cancellation between pops; cap waits even if a caller asks for more.
        let timeout = timeout.min(Duration::from_millis(100));
        let mut state = match self.ready.wait_timeout_while(state, timeout, |state| {
            !state.reset && state.units.is_empty()
        }) {
            Ok((state, _)) => state,
            Err(poisoned) => {
                let (mut state, _) = poisoned.into_inner();
                state.units.clear();
                state.reset = true;
                self.synced.store(false, Ordering::Release);
                state
            }
        };
        let reset = std::mem::take(&mut state.reset);
        if reset {
            self.synced.store(false, Ordering::Release);
        }
        (reset, state.units.pop_front())
    }

    fn lock(&self) -> MutexGuard<'_, QueueState> {
        match self.state.lock() {
            Ok(state) => state,
            Err(poisoned) => {
                // A panicked producer may have interrupted assembly; never reuse its references.
                let mut state = poisoned.into_inner();
                state.units.clear();
                state.reset = true;
                self.synced.store(false, Ordering::Release);
                state
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overflow_discards_reference_backlog_and_retains_fresh_unit() {
        let queue = DecodeQueue::default();
        queue.synced.store(true, Ordering::Release);
        for value in 0..8 {
            assert!(queue.push(vec![value]));
        }
        assert!(!queue.push(vec![9]));
        assert!(!queue.synced.load(Ordering::Acquire));
        assert_eq!(queue.pop(Duration::ZERO), (true, Some(vec![9])));
        assert_eq!(queue.pop(Duration::ZERO), (false, None));
    }

    #[test]
    fn rejects_oversized_unit_and_invalidates_old_references() {
        let queue = DecodeQueue::default();
        queue.push(vec![1]);
        assert!(!queue.push(vec![0; 1024 * 1024 + 1]));
        assert_eq!(queue.pop(Duration::ZERO), (true, None));
        assert!(queue.push(vec![2]));
        assert_eq!(queue.pop(Duration::ZERO), (false, Some(vec![2])));
    }

    #[test]
    fn reset_is_consumed_once_and_wakes_waiter() {
        let queue = Arc::new(DecodeQueue::default());
        let waiting = queue.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            tx.send(waiting.pop(Duration::from_secs(3))).unwrap();
        });
        queue.synced.store(true, Ordering::Release);
        queue.reset();
        assert_eq!(
            rx.recv_timeout(Duration::from_millis(500)).unwrap(),
            (true, None)
        );
        assert!(!queue.synced.load(Ordering::Acquire));
        assert_eq!(queue.pop(Duration::ZERO), (false, None));
        worker.join().unwrap();
    }

    #[test]
    fn reset_during_decode_prevents_restoring_stale_synchronization() {
        let queue = DecodeQueue::default();
        queue.push(vec![1]);
        assert_eq!(queue.pop(Duration::ZERO), (false, Some(vec![1])));
        queue.reset();
        assert!(!queue.finish_frame());
        assert!(!queue.synced.load(Ordering::Acquire));
        assert_eq!(queue.pop(Duration::ZERO), (true, None));
        assert!(queue.finish_frame());
        assert!(queue.synced.load(Ordering::Acquire));
    }
}
