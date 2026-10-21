use std::collections::HashMap;
use std::sync::Mutex;
use tokio_util::sync::CancellationToken;

/// Tracks the cancellation token for each in-flight compression job so the
/// frontend can cancel a job by its id.
pub struct JobState {
    tokens: Mutex<HashMap<String, CancellationToken>>,
}

impl JobState {
    pub fn new() -> Self {
        Self {
            tokens: Mutex::new(HashMap::new()),
        }
    }

    /// Register a fresh token for `job_id` and return it.
    pub fn token_for(&self, job_id: &str) -> CancellationToken {
        let token = CancellationToken::new();
        self.tokens
            .lock()
            .expect("job token map poisoned")
            .insert(job_id.to_string(), token.clone());
        token
    }

    /// Signal cancellation for `job_id` (no-op if unknown).
    pub fn cancel(&self, job_id: &str) {
        if let Some(token) = self
            .tokens
            .lock()
            .expect("job token map poisoned")
            .get(job_id)
        {
            token.cancel();
        }
    }

    /// Drop the token once the job has finished.
    pub fn remove(&self, job_id: &str) {
        self.tokens
            .lock()
            .expect("job token map poisoned")
            .remove(job_id);
    }
}
