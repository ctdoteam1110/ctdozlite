//! Cached account profiles and request ownership for the Settings account panel.

use crate::account_manager::AccountProfile;

#[derive(Default)]
pub(super) struct SavedAccountsState {
    pub profiles: Vec<AccountProfile>,
    pub error: Option<String>,
    pending: Option<u64>,
    next_request: u64,
    loaded: bool,
}

impl SavedAccountsState {
    pub fn invalidate(&mut self) {
        self.profiles.clear();
        self.error = None;
        self.pending = None;
        self.loaded = false;
    }

    pub fn request(&mut self) -> Option<u64> {
        if self.loaded || self.pending.is_some() {
            return None;
        }
        self.next_request = self.next_request.wrapping_add(1);
        self.pending = Some(self.next_request);
        self.pending
    }

    pub fn loading(&self) -> bool {
        self.pending.is_some()
    }

    pub fn finish(&mut self, request_id: u64, result: Result<Vec<AccountProfile>, String>) {
        if self.pending != Some(request_id) {
            return;
        }
        self.pending = None;
        self.loaded = true;
        match result {
            Ok(profiles) => {
                self.profiles = profiles;
                self.error = None;
            }
            Err(error) => self.error = Some(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repaint_requests_are_coalesced_until_explicit_refresh() {
        let mut state = SavedAccountsState::default();
        let first = state.request().unwrap();
        for _ in 0..100 {
            assert!(state.request().is_none());
        }
        state.finish(first, Ok(vec![]));
        for _ in 0..100 {
            assert!(state.request().is_none());
        }
        state.invalidate();
        assert!(state.request().is_some());
    }

    #[test]
    fn late_account_response_cannot_replace_new_account_profiles() {
        let mut state = SavedAccountsState::default();
        let old = state.request().unwrap();
        state.invalidate();
        let current = state.request().unwrap();
        state.finish(old, Err("old account failure".into()));
        assert!(state.loading());
        assert!(state.error.is_none());
        let profile = AccountProfile {
            uin: "user-1".into(),
            display_name: "Test".into(),
            avatar: None,
            last_active_ts: 0,
            is_current: true,
        };
        state.finish(current, Ok(vec![profile.clone()]));
        assert_eq!(state.profiles, vec![profile]);
        assert!(!state.loading());
    }

    #[test]
    fn failure_is_terminal_and_can_be_retried_explicitly() {
        let mut state = SavedAccountsState::default();
        let request = state.request().unwrap();
        state.finish(request, Err("Unavailable".into()));
        assert!(!state.loading());
        assert_eq!(state.error.as_deref(), Some("Unavailable"));
        assert!(state.request().is_none());
        state.invalidate();
        assert!(state.request().is_some());
    }
}
