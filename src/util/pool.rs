//! The vision engine's worker threads.
//!
//! One pool for the whole process, built on first use: at most one fewer
//! thread than the machine has cores — the game keeps a core to itself —
//! and no more than eight, which is more than there are things to look
//! for at once. On Windows the workers run at below-normal priority, so
//! when the game and the companion want the same core at the same moment,
//! the game gets it.

use std::sync::OnceLock;

use rayon::ThreadPool;

/// Most worker threads, however many cores there are.
const MOST_WORKERS: usize = 8;

/// The pool, built on first use.
pub fn pool() -> &'static ThreadPool {
    static POOL: OnceLock<ThreadPool> = OnceLock::new();
    POOL.get_or_init(|| {
        let cores = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(2);
        let workers = cores.saturating_sub(1).clamp(1, MOST_WORKERS);
        rayon::ThreadPoolBuilder::new()
            .num_threads(workers)
            .thread_name(|i| format!("syrup-vision-{i}"))
            .start_handler(|_| lower_priority())
            .build()
            .expect("a thread pool")
    })
}

/// How many workers the pool has.
pub fn workers() -> usize {
    pool().current_num_threads()
}

/// Below-normal priority for the calling thread (Windows; elsewhere the
/// workers run at the process's priority).
fn lower_priority() {
    #[cfg(windows)]
    unsafe {
        use windows::Win32::System::Threading::{
            GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_BELOW_NORMAL,
        };
        let _ = SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pool_leaves_the_game_a_core() {
        let cores = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(2);
        let workers = workers();
        assert!((1..=MOST_WORKERS).contains(&workers));
        assert!(cores <= 1 || workers < cores);
        // Work runs on the pool's own threads.
        let name = pool().install(|| std::thread::current().name().map(str::to_string));
        assert!(
            name.as_deref()
                .is_some_and(|n| n.starts_with("syrup-vision-")),
            "{name:?}"
        );
    }
}
