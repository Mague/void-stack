//! Per-session memory of what search already sent.
//!
//! An agent that asks five broad questions in one session tends to get the
//! same README section or CLAUDE.md block back five times. The server
//! instance lives as long as the session (one stdio process, or one HTTP
//! session), so it remembers which chunks it sent and prints a one-line
//! reference instead of the content the next time.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// A chunk is "already sent" for this long.
pub(crate) const SEEN_TTL: Duration = Duration::from_secs(2 * 60 * 60);

#[derive(Clone, Default)]
pub(crate) struct SeenChunks {
    inner: Arc<Mutex<HashMap<String, Instant>>>,
}

fn key(project: &str, file: &str, start: usize, end: usize) -> String {
    format!(
        "{}\u{0}{}\u{0}{}\u{0}{}",
        project.to_lowercase(),
        file,
        start,
        end
    )
}

impl SeenChunks {
    /// Was this chunk sent within the TTL?
    pub(crate) fn contains(&self, project: &str, file: &str, start: usize, end: usize) -> bool {
        self.contains_at(project, file, start, end, Instant::now())
    }

    fn contains_at(
        &self,
        project: &str,
        file: &str,
        start: usize,
        end: usize,
        now: Instant,
    ) -> bool {
        let map = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        map.get(&key(project, file, start, end))
            .is_some_and(|t| now.duration_since(*t) < SEEN_TTL)
    }

    /// Remember a chunk as sent now.
    pub(crate) fn mark(&self, project: &str, file: &str, start: usize, end: usize) {
        let now = Instant::now();
        let mut map = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        // Keep the map from growing without bound in very long sessions.
        if map.len() > 5000 {
            map.retain(|_, t| now.duration_since(*t) < SEEN_TTL);
        }
        map.insert(key(project, file, start, end), now);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remembers_per_project_and_range() {
        let s = SeenChunks::default();
        assert!(!s.contains("p", "a.rs", 1, 10));
        s.mark("p", "a.rs", 1, 10);
        assert!(
            s.contains("P", "a.rs", 1, 10),
            "project names are case-insensitive"
        );
        assert!(!s.contains("p", "a.rs", 1, 11));
        assert!(!s.contains("q", "a.rs", 1, 10));
    }

    #[test]
    fn expires_after_ttl() {
        let s = SeenChunks::default();
        s.mark("p", "a.rs", 1, 10);
        let later = Instant::now() + SEEN_TTL + Duration::from_secs(1);
        assert!(!s.contains_at("p", "a.rs", 1, 10, later));
    }

    #[test]
    fn clones_share_state() {
        let s = SeenChunks::default();
        let c = s.clone();
        c.mark("p", "x", 1, 2);
        assert!(s.contains("p", "x", 1, 2));
    }
}
