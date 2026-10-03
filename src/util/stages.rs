//! Per-stage timing of the vision path, from the tracing spans it opens.
//!
//! The hot path wraps each of its stages in a `tracing` span at `TRACE`
//! level (`vision.hud.geometry`, `vision.motion`, `sight.things`, …). In a
//! normal run those spans are filtered out before they cost anything; a
//! benchmark or a debug view installs a [`StageRecorder`] layer, which
//! enables them and keeps every span's duration under its name, so the
//! time a frame took can be laid out stage by stage: how many times each
//! ran, and its mean, median and 95th percentile.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use tracing::span::Id;
use tracing::{Metadata, Subscriber};
use tracing_subscriber::layer::{Context, Layer};
use tracing_subscriber::registry::LookupSpan;

/// When a span was entered; kept in the span's extensions until it exits.
struct Entered(Instant);

/// A tracing layer that records how long each span took, by span name.
#[derive(Clone, Default)]
pub struct StageRecorder {
    /// Durations in milliseconds, per span name, in the order they ended.
    samples: Arc<Mutex<BTreeMap<&'static str, Vec<f64>>>>,
}

impl StageRecorder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Every stage seen so far, with its statistics.
    pub fn stats(&self) -> Vec<StageStats> {
        let samples = self.samples.lock().unwrap_or_else(|e| e.into_inner());
        samples
            .iter()
            .map(|(name, durations)| StageStats::from_samples(name, durations))
            .collect()
    }

    /// Forget everything recorded so far.
    pub fn reset(&self) {
        self.samples
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
    }

    /// The durations recorded for one stage, in milliseconds.
    pub fn samples(&self, name: &str) -> Vec<f64> {
        self.samples
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(name)
            .cloned()
            .unwrap_or_default()
    }
}

impl<S> Layer<S> for StageRecorder
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn enabled(&self, metadata: &Metadata<'_>, _ctx: Context<'_, S>) -> bool {
        // Spans only; events are someone else's business.
        metadata.is_span()
    }

    fn on_enter(&self, id: &Id, ctx: Context<'_, S>) {
        if let Some(span) = ctx.span(id) {
            span.extensions_mut().replace(Entered(Instant::now()));
        }
    }

    fn on_exit(&self, id: &Id, ctx: Context<'_, S>) {
        let Some(span) = ctx.span(id) else { return };
        let entered = span.extensions_mut().remove::<Entered>();
        if let Some(Entered(at)) = entered {
            let ms = at.elapsed().as_secs_f64() * 1e3;
            self.samples
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .entry(span.name())
                .or_default()
                .push(ms);
        }
    }
}

/// How one stage did over a run.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct StageStats {
    pub name: String,
    pub count: usize,
    /// All in milliseconds.
    pub mean: f64,
    pub p50: f64,
    pub p95: f64,
    pub max: f64,
    pub total: f64,
}

impl StageStats {
    pub fn from_samples(name: &str, samples: &[f64]) -> StageStats {
        let mut sorted = samples.to_vec();
        sorted.sort_by(|a, b| a.total_cmp(b));
        let total: f64 = sorted.iter().sum();
        StageStats {
            name: name.to_string(),
            count: sorted.len(),
            mean: if sorted.is_empty() {
                0.0
            } else {
                total / sorted.len() as f64
            },
            p50: percentile(&sorted, 0.50),
            p95: percentile(&sorted, 0.95),
            max: sorted.last().copied().unwrap_or(0.0),
            total,
        }
    }
}

/// The value `q` of the way through `sorted` (nearest rank), 0 when empty.
pub fn percentile(sorted: &[f64], q: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let rank = ((sorted.len() - 1) as f64 * q).round() as usize;
    sorted[rank.min(sorted.len() - 1)]
}

#[cfg(test)]
mod tests {
    use super::*;
    use tracing_subscriber::prelude::*;

    #[test]
    fn spans_are_timed_by_name() {
        let recorder = StageRecorder::new();
        let subscriber = tracing_subscriber::registry().with(recorder.clone());
        tracing::subscriber::with_default(subscriber, || {
            for _ in 0..3 {
                let _outer = tracing::trace_span!("bench.outer").entered();
                {
                    let _inner = tracing::trace_span!("bench.inner").entered();
                    std::thread::sleep(std::time::Duration::from_millis(2));
                }
            }
        });
        let stats = recorder.stats();
        let inner = stats.iter().find(|s| s.name == "bench.inner").unwrap();
        let outer = stats.iter().find(|s| s.name == "bench.outer").unwrap();
        assert_eq!((inner.count, outer.count), (3, 3));
        assert!(inner.p50 >= 2.0, "{inner:?}");
        assert!(outer.mean >= inner.mean, "{outer:?} vs {inner:?}");
        assert_eq!(recorder.samples("bench.inner").len(), 3);
        recorder.reset();
        assert!(recorder.stats().is_empty());
    }

    #[test]
    fn percentiles_use_the_nearest_rank() {
        let sorted: Vec<f64> = (1..=100).map(|v| v as f64).collect();
        assert_eq!(percentile(&sorted, 0.5), 51.0);
        assert_eq!(percentile(&sorted, 0.95), 95.0);
        assert_eq!(percentile(&[], 0.5), 0.0);
        let one = StageStats::from_samples("x", &[4.0]);
        assert_eq!((one.p50, one.p95, one.max, one.count), (4.0, 4.0, 4.0, 1));
    }
}
