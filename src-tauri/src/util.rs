use std::sync::{Mutex, MutexGuard};

/// Extension trait for Mutex that recovers from poisoning instead of panicking.
/// If a thread panicked while holding the lock, we take the data anyway
/// rather than cascading the panic to every subsequent lock attempt.
pub(crate) trait MutexExt<T> {
    fn lock_or_recover(&self) -> MutexGuard<'_, T>;
}

impl<T> MutexExt<T> for Mutex<T> {
    fn lock_or_recover(&self) -> MutexGuard<'_, T> {
        self.lock().unwrap_or_else(|e| {
            eprintln!("Clyde: mutex poisoned, recovering");
            e.into_inner()
        })
    }
}
