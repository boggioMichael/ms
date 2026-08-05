//! Time utilities for game simulation.

use crate::types::Time;
use std::time::Duration;

/// Advance a Time by a Duration returning a new Time. Kept as a thin helper so
/// tests and other parts of the codebase can centralize temporal operations.
pub fn advance(t: Time, d: Duration) -> Option<Time> {
    t.checked_add(d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Time;
    use std::time::Duration;

    #[test]
    fn advance_time() {
        let now = Time::now();
        let later = advance(now, Duration::from_secs(1)).unwrap();
        assert!(later > now);
    }
}
