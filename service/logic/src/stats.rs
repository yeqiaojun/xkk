use std::collections::{HashMap, hash_map::Entry};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use xkk_common::{LatencyRecorder, LatencyStats};

pub const LATENCY_BUCKETS_US: [u64; 18] = [
    10,
    25,
    50,
    100,
    250,
    500,
    1_000,
    2_500,
    5_000,
    10_000,
    15_000,
    20_000,
    30_000,
    50_000,
    75_000,
    100_000,
    250_000,
    u64::MAX,
];

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LatencyHistogram {
    pub counts: [u64; LATENCY_BUCKETS_US.len()],
}

impl LatencyHistogram {
    pub fn count(&self) -> u64 {
        self.counts.iter().sum()
    }

    pub fn percentile_micros(&self, percentile: f64) -> Option<u64> {
        let count = self.count();
        if count == 0 {
            return None;
        }
        assert!((0.0..=100.0).contains(&percentile));

        let rank = ((count as f64 * percentile / 100.0).ceil() as u64).max(1);
        let mut seen = 0;
        for (bound, count) in LATENCY_BUCKETS_US.iter().zip(self.counts) {
            seen += count;
            if seen >= rank {
                return Some(*bound);
            }
        }
        unreachable!("logic latency histogram count changed while reading a snapshot")
    }
}

pub(crate) struct AtomicHistogram {
    counts: [AtomicU64; LATENCY_BUCKETS_US.len()],
}

impl AtomicHistogram {
    pub(crate) fn new() -> Self {
        Self {
            counts: std::array::from_fn(|_| AtomicU64::new(0)),
        }
    }

    #[inline]
    pub(crate) fn record(&self, elapsed: Duration) {
        let micros = elapsed.as_micros().min(u128::from(u64::MAX)) as u64;
        let bucket = LATENCY_BUCKETS_US.partition_point(|&bound| bound < micros);
        self.counts[bucket].fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn snapshot(&self) -> LatencyHistogram {
        LatencyHistogram {
            counts: std::array::from_fn(|index| self.counts[index].load(Ordering::Relaxed)),
        }
    }
}

pub(crate) struct StatsInner {
    pub(crate) inflight_calls_high_water: AtomicU64,
    pub(crate) inflight_kib_high_water: AtomicU64,
    pub(crate) queued: AtomicU64,
    pub(crate) queued_high_water: AtomicU64,
    pub(crate) active_gids: AtomicU64,
    pub(crate) active_gids_high_water: AtomicU64,
    pub(crate) dirty_players: AtomicU64,
    pub(crate) dirty_slots_high_water: AtomicU64,
    pub(crate) accepted: AtomicU64,
    pub(crate) completed: AtomicU64,
    pub(crate) load_calls: AtomicU64,
    pub(crate) load_failed: AtomicU64,
    pub(crate) preload_calls: AtomicU64,
    pub(crate) preload_failed: AtomicU64,
    pub(crate) save_calls: AtomicU64,
    pub(crate) save_failed: AtomicU64,
    pub(crate) rejected_calls: AtomicU64,
    pub(crate) rejected_kib: AtomicU64,
    pub(crate) rejected_gid: AtomicU64,
    pub(crate) rejected_dirty: AtomicU64,
    pub(crate) rejected_draining: AtomicU64,
    pub(crate) queue_latency: AtomicHistogram,
    pub(crate) load_latency: AtomicHistogram,
    pub(crate) run_latency: AtomicHistogram,
    pub(crate) preload_latency: AtomicHistogram,
    pub(crate) save_latency: AtomicHistogram,
    pub(crate) total_latency: AtomicHistogram,
    pub(crate) flush_latency: AtomicHistogram,
    dirty_since: Mutex<HashMap<u64, Instant>>,
}

impl StatsInner {
    pub(crate) fn new() -> Self {
        Self {
            inflight_calls_high_water: AtomicU64::new(0),
            inflight_kib_high_water: AtomicU64::new(0),
            queued: AtomicU64::new(0),
            queued_high_water: AtomicU64::new(0),
            active_gids: AtomicU64::new(0),
            active_gids_high_water: AtomicU64::new(0),
            dirty_players: AtomicU64::new(0),
            dirty_slots_high_water: AtomicU64::new(0),
            accepted: AtomicU64::new(0),
            completed: AtomicU64::new(0),
            load_calls: AtomicU64::new(0),
            load_failed: AtomicU64::new(0),
            preload_calls: AtomicU64::new(0),
            preload_failed: AtomicU64::new(0),
            save_calls: AtomicU64::new(0),
            save_failed: AtomicU64::new(0),
            rejected_calls: AtomicU64::new(0),
            rejected_kib: AtomicU64::new(0),
            rejected_gid: AtomicU64::new(0),
            rejected_dirty: AtomicU64::new(0),
            rejected_draining: AtomicU64::new(0),
            queue_latency: AtomicHistogram::new(),
            load_latency: AtomicHistogram::new(),
            run_latency: AtomicHistogram::new(),
            preload_latency: AtomicHistogram::new(),
            save_latency: AtomicHistogram::new(),
            total_latency: AtomicHistogram::new(),
            flush_latency: AtomicHistogram::new(),
            dirty_since: Mutex::new(HashMap::new()),
        }
    }

    pub(crate) fn mark_dirty(&self, player_id: u64, dirty: bool) {
        let mut players = self
            .dirty_since
            .lock()
            .expect("logic dirty registry mutex poisoned");
        if dirty {
            if let Entry::Vacant(entry) = players.entry(player_id) {
                entry.insert(Instant::now());
                self.dirty_players.fetch_add(1, Ordering::Relaxed);
            }
        } else if players.remove(&player_id).is_some() {
            self.dirty_players.fetch_sub(1, Ordering::Relaxed);
        }
    }

    pub(crate) fn oldest_dirty_age(&self) -> Duration {
        let players = self
            .dirty_since
            .lock()
            .expect("logic dirty registry mutex poisoned");
        let Some(oldest) = players.values().min() else {
            return Duration::ZERO;
        };
        oldest.elapsed()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LogicStats {
    pub inflight_calls: u64,
    pub inflight_calls_high_water: u64,
    pub inflight_kib: u64,
    pub inflight_kib_high_water: u64,
    pub queued: u64,
    pub queued_high_water: u64,
    pub active_gids: u64,
    pub active_gids_high_water: u64,
    pub dirty_players: u64,
    pub dirty_slots: u64,
    pub dirty_slots_high_water: u64,
    pub accepted: u64,
    pub completed: u64,
    pub load_calls: u64,
    pub load_failed: u64,
    pub preload_calls: u64,
    pub preload_failed: u64,
    pub save_calls: u64,
    pub save_failed: u64,
    pub rejected_calls: u64,
    pub rejected_kib: u64,
    pub rejected_gid: u64,
    pub rejected_dirty: u64,
    pub rejected_draining: u64,
    pub queue_latency: LatencyHistogram,
    pub load_latency: LatencyHistogram,
    pub run_latency: LatencyHistogram,
    pub preload_latency: LatencyHistogram,
    pub save_latency: LatencyHistogram,
    pub total_latency: LatencyHistogram,
    pub flush_latency: LatencyHistogram,
    pub oldest_dirty_age: Duration,
    pub cache: xlru::Stats,
}

#[inline]
pub(crate) fn update_high_water(high_water: &AtomicU64, value: u64) {
    high_water.fetch_max(value, Ordering::Relaxed);
}

#[derive(Default)]
pub(crate) struct LoginMetrics {
    pub(crate) mongo_find: LatencyRecorder,
    pub(crate) mongo_create: LatencyRecorder,
    pub(crate) runtime_wait: LatencyRecorder,
    pub(crate) redis_owner: LatencyRecorder,
    pub(crate) total: LatencyRecorder,
}

#[derive(Clone, Copy)]
pub(crate) struct LoginStats {
    pub(crate) mongo_find: LatencyStats,
    pub(crate) mongo_create: LatencyStats,
    pub(crate) runtime_wait: LatencyStats,
    pub(crate) redis_owner: LatencyStats,
    pub(crate) total: LatencyStats,
}

impl LoginMetrics {
    pub(crate) fn snapshot(&self) -> LoginStats {
        LoginStats {
            mongo_find: self.mongo_find.snapshot(),
            mongo_create: self.mongo_create.snapshot(),
            runtime_wait: self.runtime_wait.snapshot(),
            redis_owner: self.redis_owner.snapshot(),
            total: self.total.snapshot(),
        }
    }
}
