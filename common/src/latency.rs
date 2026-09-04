use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

const BUCKETS_US: [u64; 21] = [
    10,
    25,
    50,
    100,
    250,
    500,
    1_000,
    2_500,
    5_000,
    7_500,
    10_000,
    15_000,
    20_000,
    30_000,
    50_000,
    75_000,
    100_000,
    150_000,
    250_000,
    500_000,
    u64::MAX,
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LatencyStats {
    counts: [u64; BUCKETS_US.len()],
    total_micros: u64,
    pub max_micros: u64,
}

impl LatencyStats {
    pub fn count(&self) -> u64 {
        self.counts.iter().sum()
    }

    pub fn average_micros(&self) -> Option<u64> {
        let count = self.count();
        (count != 0).then(|| self.total_micros / count)
    }

    pub fn percentile_micros(&self, percentile: f64) -> Option<u64> {
        let count = self.count();
        if count == 0 {
            return None;
        }
        assert!((0.0..=100.0).contains(&percentile));

        let rank = ((count as f64 * percentile / 100.0).ceil() as u64).max(1);
        let mut seen = 0;
        for (bound, count) in BUCKETS_US.iter().zip(self.counts) {
            seen += count;
            if seen >= rank {
                return Some(*bound);
            }
        }
        unreachable!("latency histogram count changed while reading a snapshot")
    }
}

pub struct LatencyRecorder {
    counts: [AtomicU64; BUCKETS_US.len()],
    total_micros: AtomicU64,
    max_micros: AtomicU64,
}

impl Default for LatencyRecorder {
    fn default() -> Self {
        Self { counts: std::array::from_fn(|_| AtomicU64::new(0)), total_micros: AtomicU64::new(0), max_micros: AtomicU64::new(0) }
    }
}

impl LatencyRecorder {
    #[inline]
    pub fn record(&self, elapsed: Duration) {
        let micros = elapsed.as_micros().min(u128::from(u64::MAX)) as u64;
        let bucket = BUCKETS_US.partition_point(|&bound| bound < micros);
        self.counts[bucket].fetch_add(1, Ordering::Relaxed);
        self.total_micros.fetch_add(micros, Ordering::Relaxed);
        self.max_micros.fetch_max(micros, Ordering::Relaxed);
    }

    pub fn snapshot(&self) -> LatencyStats {
        LatencyStats {
            counts: std::array::from_fn(|index| self.counts[index].load(Ordering::Relaxed)),
            total_micros: self.total_micros.load(Ordering::Relaxed),
            max_micros: self.max_micros.load(Ordering::Relaxed),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latency_recorder_reports_average_percentile_and_max() {
        let recorder = LatencyRecorder::default();
        recorder.record(Duration::from_micros(100));
        recorder.record(Duration::from_micros(900));

        let stats = recorder.snapshot();
        assert_eq!(stats.count(), 2);
        assert_eq!(stats.average_micros(), Some(500));
        assert_eq!(stats.percentile_micros(50.0), Some(100));
        assert_eq!(stats.percentile_micros(99.0), Some(1_000));
        assert_eq!(stats.max_micros, 900);
    }
}
