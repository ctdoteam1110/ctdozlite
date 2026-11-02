//! Bounded keyed latest values with one outstanding payload-free worker wake.
use std::{collections::BTreeMap, sync::Mutex};

struct Pending<K, V> {
    values: BTreeMap<K, V>,
    awake: bool,
    token: u64,
}
/// An atomic update failed before changing the retained value or its wake token.
#[derive(Debug, PartialEq, Eq)]
pub enum UpdateError<E> { Full, Rejected(E) }

/// Coalesce replaceable snapshots without retaining a payload in an unbounded event queue.
pub struct LatestSnapshot<K, V> {
    capacity: usize,
    pending: Mutex<Pending<K, V>>,
}
impl<K: Ord, V> LatestSnapshot<K, V> {
    /// Construct a bounded mailbox; publication never waits for disk or an async consumer.
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            pending: Mutex::new(Pending {
                values: BTreeMap::new(),
                awake: false,
                token: 0,
            }),
        }
    }
    /// Replace one key. `Ok(true)` asks the caller to enqueue the single payload-free wake.
    ///
    /// # Errors
    /// Returns the unsaved value when a new key would exceed capacity; existing keys still replace.
    pub fn publish(&self, key: K, value: V) -> Result<bool, V> {
        let mut pending = self.pending.lock().unwrap_or_else(|e| e.into_inner());
        if !pending.values.contains_key(&key) && pending.values.len() >= self.capacity {
            return Err(value);
        }
        pending.values.insert(key, value);
        let wake = !pending.awake;
        if wake {
            pending.token = pending.token.wrapping_add(1);
        }
        pending.awake = true;
        Ok(wake)
    }
    /// Atomically transform one bounded value without acknowledging any retained payload.
    /// The closure must perform only short bounded in-memory work, never I/O.
    ///
    /// # Errors
    /// Returns Full before invoking the closure when a new key exceeds capacity.
    /// A rejected transform leaves the previous value and wake token unchanged.
    pub fn update<E>(&self, key: K, update: impl FnOnce(Option<&V>) -> Result<V,E>) -> Result<bool, UpdateError<E>> {
        let mut pending = self.pending.lock().unwrap_or_else(|e| e.into_inner());
        if !pending.values.contains_key(&key) && pending.values.len() >= self.capacity { return Err(UpdateError::Full); }
        let value = update(pending.values.get(&key)).map_err(UpdateError::Rejected)?;
        pending.values.insert(key,value);
        let wake = !pending.awake;
        if wake { pending.token = pending.token.wrapping_add(1); }
        pending.awake = true;
        Ok(wake)
    }
    /// Take the latest batch and release the wake slot atomically with removal.
    pub fn take(&self) -> BTreeMap<K, V> {
        let mut pending = self.pending.lock().unwrap_or_else(|e| e.into_inner());
        pending.awake = false;
        std::mem::take(&mut pending.values)
    }
    /// Clone the bounded batch without acknowledging it; failed writes remain available for retry.
    pub fn snapshot(&self) -> BTreeMap<K, V>
    where
        K: Clone,
        V: Clone,
    {
        self.pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .values
            .clone()
    }
    /// Identify the outstanding wake so a late canceled consumer cannot release a newer one.
    pub fn wake_token(&self) -> u64 {
        self.pending.lock().unwrap_or_else(|e| e.into_inner()).token
    }
    /// Release only the wake captured by this consumer, including an unpolled canceled future.
    pub fn release_wake_for(&self, token: u64) {
        let mut pending = self.pending.lock().unwrap_or_else(|e| e.into_inner());
        if pending.token == token {
            pending.awake = false;
        }
    }
    /// Release the outstanding wake after a consumer pass, including a failed pass.
    pub fn release_wake(&self) {
        self.pending.lock().unwrap_or_else(|e| e.into_inner()).awake = false;
    }
    /// Remove a superseded revision while leaving a later edit untouched.
    pub fn remove_if(&self, key: &K, predicate: impl FnOnce(&V) -> bool) {
        let mut pending = self.pending.lock().unwrap_or_else(|e| e.into_inner());
        if pending.values.get(key).is_some_and(predicate) {
            pending.values.remove(key);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounded_latest_snapshot_coalesces_one_wake_and_preserves_newer_edit() {
        let queue = LatestSnapshot::new(2);
        assert_eq!(queue.publish("A", 1), Ok(true));
        for revision in 2..10000 {
            assert_eq!(queue.publish("A", revision), Ok(false));
        }
        assert_eq!(queue.publish("B", 5), Ok(false));
        assert_eq!(queue.publish("C", 1), Err(1));
        queue.remove_if(&"A", |revision| *revision <= 3);
        let batch = queue.take();
        assert_eq!(batch["A"], 9999);
        assert_eq!(batch.len(), 2);
        assert_eq!(queue.publish("C", 1), Ok(true));
    }
    #[test]
    fn atomic_update_retains_old_values_and_tokens_on_failure() {
        let queue=LatestSnapshot::new(1);
        assert_eq!(queue.update("A", |_| Ok::<_,()>(vec![1])),Ok(true));
        let token=queue.wake_token();
        assert_eq!(queue.update("A", |_| Err::<Vec<i32>,_>("rejected")),Err(UpdateError::Rejected("rejected")));
        assert_eq!(queue.update("B", |_| panic!("full closure must not run")),Err::<bool,UpdateError<()>>(UpdateError::Full));
        assert_eq!(queue.snapshot()["A"],vec![1]);assert_eq!(queue.wake_token(),token);
        assert_eq!(queue.update("A", |old| {let mut value=old.unwrap().clone();value.push(2);Ok::<_,()>(value)}),Ok(false));
        assert_eq!(queue.snapshot()["A"],vec![1,2]);
    }

}
