use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use dashmap::DashSet;
use xframe::xmongo::{self, Collection, mongodb::bson::Document};
use xkk_common::{LatencyRecorder, LatencyStats};
use xkk_persist::{PublicPlayer, load_model, save_model, save_models};
use xkk_protocol::pb;
use xlru::{Options, XlruCache};

// One Public process retains a large, long-lived working set. The deployment
// must size Public ownership so active players do not reach this eviction edge.
const PLAYER_CACHE_CAPACITY: usize = 500_000;
const PLAYER_CACHE_TTL: Duration = Duration::from_secs(24 * 60 * 60);
const PLAYER_CACHE_SHARDS: usize = 128;
const PLAYER_SAVE_BATCH_SIZE: usize = 1_000;

type Cache = XlruCache<i64, Arc<PublicPlayer>, xmongo::Error>;
pub(crate) type CacheError = xlru::Error<xmongo::Error>;

#[derive(Clone)]
pub(crate) struct Players {
    inner: Arc<PlayersInner>,
}

struct PlayersInner {
    cache: Cache,
    dirty: Arc<DashSet<i64>>,
    metrics: Arc<Metrics>,
}

#[derive(Default)]
struct Metrics {
    load_calls: AtomicU64,
    load_failed: AtomicU64,
    save_players: AtomicU64,
    save_failed: AtomicU64,
    load_latency: LatencyRecorder,
    save_latency: LatencyRecorder,
    flush_latency: LatencyRecorder,
}

#[derive(Clone, Copy)]
pub(crate) struct PlayersStats {
    pub cache: xlru::Stats,
    pub dirty_players: usize,
    pub load_calls: u64,
    pub load_failed: u64,
    pub save_players: u64,
    pub save_failed: u64,
    pub load_latency: LatencyStats,
    pub save_latency: LatencyStats,
    pub flush_latency: LatencyStats,
}

impl Players {
    pub(crate) fn new(collection: Collection<Document>) -> Self {
        let dirty = Arc::new(DashSet::new());
        let metrics = Arc::new(Metrics::default());

        let load_collection = collection.clone();
        let load_metrics = metrics.clone();
        let single_collection = collection.clone();
        let single_metrics = metrics.clone();
        let single_dirty = dirty.clone();
        let batch_metrics = metrics.clone();
        let options = Options::new()
            .with_ttl(PLAYER_CACHE_TTL)
            .with_sliding(true)
            .with_shards(PLAYER_CACHE_SHARDS)
            .with_batch_save_count(PLAYER_SAVE_BATCH_SIZE)
            .with_loader(move |gid| {
                let collection = load_collection.clone();
                let metrics = load_metrics.clone();
                async move {
                    metrics.load_calls.fetch_add(1, Ordering::Relaxed);
                    let started = Instant::now();
                    let result = load_model::<pb::PublicPlayerData>(&collection, gid).await;
                    metrics.load_latency.record(started.elapsed());
                    match result {
                        Ok(Some(data)) => Ok(Arc::new(PublicPlayer::new(gid, data))),
                        // Reads do not create Mongo documents. The first real
                        // business mutation marks this empty aggregate dirty.
                        Ok(None) => Ok(Arc::new(PublicPlayer::empty(gid))),
                        Err(error) => {
                            metrics.load_failed.fetch_add(1, Ordering::Relaxed);
                            Err(error)
                        }
                    }
                }
            })
            .with_saver(move |gid, player: Arc<PublicPlayer>| {
                let collection = single_collection.clone();
                let metrics = single_metrics.clone();
                let dirty = single_dirty.clone();
                async move {
                    save_single(&collection, &metrics, gid, &player).await;
                    player.remove_registration_if_clean(|| {
                        dirty.remove(&gid);
                    });
                    Ok::<(), xmongo::Error>(())
                }
            })
            .with_batch_saver(move |entries: Vec<(i64, Arc<PublicPlayer>)>| {
                let collection = collection.clone();
                let metrics = batch_metrics.clone();
                async move {
                    save_batch(&collection, &metrics, entries).await;
                    Ok::<(), xmongo::Error>(())
                }
            });
        Self {
            inner: Arc::new(PlayersInner {
                cache: XlruCache::new(PLAYER_CACHE_CAPACITY, options),
                dirty,
                metrics,
            }),
        }
    }

    pub(crate) async fn read<R>(
        &self,
        gid: i64,
        read: impl FnOnce(&pb::PublicPlayerData) -> R,
    ) -> Result<R, CacheError> {
        let player = self.inner.cache.get_i64(gid).await?;
        Ok(player.read(read))
    }

    pub(crate) async fn update<R>(
        &self,
        gid: i64,
        update: impl FnOnce(&mut pb::PublicPlayerData) -> (R, bool),
    ) -> Result<R, CacheError> {
        let player = self.inner.cache.get_i64(gid).await?;
        let dirty = &self.inner.dirty;
        Ok(player.update(update, || {
            dirty.insert(gid);
        }))
    }

    pub(crate) async fn flush_dirty(&self) -> Result<usize, CacheError> {
        let gids = self.inner.dirty.iter().map(|gid| *gid).collect::<Vec<_>>();
        if gids.is_empty() {
            return Ok(0);
        }

        let started = Instant::now();
        let result = self.inner.cache.flush_keys(&gids).await;
        self.inner.metrics.flush_latency.record(started.elapsed());
        result?;

        for gid in &gids {
            if let Some(player) = self.inner.cache.peek(gid) {
                player.remove_registration_if_clean(|| {
                    self.inner.dirty.remove(gid);
                });
            } else {
                self.inner.dirty.remove(gid);
            }
        }
        Ok(gids.len())
    }

    pub(crate) fn stats(&self) -> PlayersStats {
        let metrics = &self.inner.metrics;
        PlayersStats {
            cache: self.inner.cache.stats(),
            dirty_players: self.inner.dirty.len(),
            load_calls: metrics.load_calls.load(Ordering::Relaxed),
            load_failed: metrics.load_failed.load(Ordering::Relaxed),
            save_players: metrics.save_players.load(Ordering::Relaxed),
            save_failed: metrics.save_failed.load(Ordering::Relaxed),
            load_latency: metrics.load_latency.snapshot(),
            save_latency: metrics.save_latency.snapshot(),
            flush_latency: metrics.flush_latency.snapshot(),
        }
    }
}

async fn save_single(
    collection: &Collection<Document>,
    metrics: &Metrics,
    gid: i64,
    player: &PublicPlayer,
) {
    let Some(snapshot) = player.take_dirty_snapshot() else {
        return;
    };
    metrics.save_players.fetch_add(1, Ordering::Relaxed);
    let started = Instant::now();
    let result = save_model(collection, &snapshot).await;
    metrics.save_latency.record(started.elapsed());
    if let Err(error) = result {
        metrics.save_failed.fetch_add(1, Ordering::Relaxed);
        tracing::error!(gid, %error, "Public player save failed");
    }
}

async fn save_batch(
    collection: &Collection<Document>,
    metrics: &Metrics,
    entries: Vec<(i64, Arc<PublicPlayer>)>,
) {
    let snapshots = entries
        .into_iter()
        .filter_map(|(_, player)| player.take_dirty_snapshot())
        .collect::<Vec<_>>();
    if snapshots.is_empty() {
        return;
    }

    let count = snapshots.len() as u64;
    metrics.save_players.fetch_add(count, Ordering::Relaxed);
    let started = Instant::now();
    let result = save_models(collection, snapshots).await;
    metrics.save_latency.record(started.elapsed());
    if let Err(error) = result {
        metrics.save_failed.fetch_add(count, Ordering::Relaxed);
        tracing::error!(players = count, %error, "Public player batch save failed");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_contract_is_large_sliding_and_batched() {
        assert_eq!(PLAYER_CACHE_CAPACITY, 500_000);
        assert_eq!(PLAYER_CACHE_TTL, Duration::from_secs(24 * 60 * 60));
        assert_eq!(PLAYER_CACHE_SHARDS, 128);
        assert_eq!(PLAYER_SAVE_BATCH_SIZE, 1_000);
    }
}
