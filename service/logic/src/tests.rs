use std::collections::HashMap;
use std::fmt;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Barrier, Mutex};
use std::time::Duration;

use tokio::sync::Semaphore;
use tokio::time::timeout;
use xkk_common::LatencyRecorder;
use xmongo::mongodb::{
    bson::{Bson, Document},
    options::ClientOptions,
};
use xmongo::{BsonPathGetter, BsonPathValue, Client, DataPersister};

use crate::{
    LogicCallError, LogicConfig, LogicRuntime, LogicState, Persistence, RejectReason, RuntimeState,
    ShutdownError,
};

#[derive(Debug)]
struct TestError(&'static str);

impl fmt::Display for TestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

impl std::error::Error for TestError {}

#[test]
fn latency_histogram_reports_percentile_upper_bound() {
    let histogram = LatencyRecorder::default();
    for _ in 0..98 {
        histogram.record(Duration::from_micros(10));
    }
    histogram.record(Duration::from_micros(5_000));
    histogram.record(Duration::from_micros(10_000));
    let histogram = histogram.snapshot();

    assert_eq!(histogram.count(), 100);
    assert_eq!(histogram.percentile_micros(98.0), Some(10));
    assert_eq!(histogram.percentile_micros(99.0), Some(5_000));
    assert_eq!(histogram.percentile_micros(99.9), Some(10_000));
}

#[derive(Clone, Copy, Debug, Default)]
struct Player {
    value: i64,
    generation: usize,
    dirty: bool,
}

impl LogicState for Player {
    fn is_dirty(&self) -> bool {
        self.dirty
    }
}

#[derive(Default)]
struct MongoPlayerData {
    value: i64,
}

impl BsonPathGetter for MongoPlayerData {
    fn bson_value(&self) -> xmongo::Result<Bson> {
        Ok(Bson::Document(Document::from_iter([(
            "value".to_string(),
            Bson::Int64(self.value),
        )])))
    }

    fn bson_path_value(&self, path: &str) -> xmongo::Result<BsonPathValue> {
        match path {
            "value" => Ok(BsonPathValue::Set(Bson::Int64(self.value))),
            _ => Err(xmongo::Error::InvalidBsonPath(path.to_string())),
        }
    }
}

struct MongoPlayer {
    data: DataPersister<MongoPlayerData>,
}

impl LogicState for MongoPlayer {
    fn is_dirty(&self) -> bool {
        self.data.has_changes()
    }
}

fn test_config() -> LogicConfig {
    LogicConfig {
        resident_capacity: 16,
        ttl: Duration::MAX,
        shards: 4,
        batch_save_count: 4,
        max_dirty_players: 16,
        max_inflight_calls: 128,
        max_inflight_kib: 128,
        max_calls_per_gid: 32,
        max_kib_per_gid: 32,
    }
}

fn memory_persistence(
    stored: Arc<Mutex<HashMap<i64, i64>>>,
    loads: Arc<Mutex<HashMap<i64, usize>>>,
) -> Persistence<Player, TestError> {
    let load_store = stored.clone();
    Persistence::new(
        move |gid| {
            let stored = load_store.clone();
            let loads = loads.clone();
            async move {
                let mut loads = loads.lock().unwrap();
                let generation = loads.entry(gid).or_default();
                *generation += 1;
                let value = stored.lock().unwrap().get(&gid).copied().unwrap_or(0);
                Ok(Player {
                    value,
                    generation: *generation,
                    dirty: false,
                })
            }
        },
        move |player| {
            let stored = stored.clone();
            async move {
                let value = player.with(|state| state.value);
                stored.lock().unwrap().insert(player.gid(), value);
                player.with_mut(|state| state.dirty = false);
                Ok(())
            }
        },
    )
}

async fn wait_until(mut ready: impl FnMut() -> bool) {
    timeout(Duration::from_secs(2), async {
        while !ready() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("condition did not become true");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn same_gid_is_strictly_serial() {
    let stored = Arc::new(Mutex::new(HashMap::new()));
    let loads = Arc::new(Mutex::new(HashMap::new()));
    let runtime = LogicRuntime::new(
        test_config(),
        memory_persistence(stored.clone(), loads.clone()),
    );

    let mut calls = Vec::new();
    for _ in 0..32 {
        calls.push(
            runtime
                .try_use(7, 1, |player| {
                    player.value += 1;
                    player.dirty = true;
                    player.value
                })
                .unwrap(),
        );
    }
    for (index, call) in calls.into_iter().enumerate() {
        let completed = call.await.unwrap();
        assert_eq!(completed.value, index as i64 + 1);
        assert!(completed.persistence.is_ok());
    }
    wait_until(|| runtime.stats().active_gids == 0).await;
    assert_eq!(runtime.stats().cache.set_calls, 1);

    runtime.shutdown().await.unwrap();
    assert_eq!(stored.lock().unwrap().get(&7), Some(&32));
    assert_eq!(loads.lock().unwrap().get(&7), Some(&1));
    let stats = runtime.stats();
    assert_eq!(stats.accepted, 32);
    assert_eq!(stats.completed, 32);
    assert_eq!(stats.inflight_calls, 0);
    assert_eq!(stats.inflight_kib, 0);
    assert_eq!(stats.queued, 0);
    assert_eq!(stats.active_gids, 0);
    assert_eq!(stats.dirty_players, 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn unrelated_gids_run_in_parallel() {
    let barrier = Arc::new(Barrier::new(2));
    let runtime = LogicRuntime::new(
        test_config(),
        memory_persistence(
            Arc::new(Mutex::new(HashMap::new())),
            Arc::new(Mutex::new(HashMap::new())),
        ),
    );

    let first_barrier = barrier.clone();
    let first = runtime
        .try_use(1, 1, move |_| {
            first_barrier.wait();
            1
        })
        .unwrap();
    let second_barrier = barrier.clone();
    let second = runtime
        .try_use(2, 1, move |_| {
            second_barrier.wait();
            2
        })
        .unwrap();

    let (first, second) = timeout(Duration::from_secs(2), async {
        tokio::join!(first, second)
    })
    .await
    .expect("different gids were head-of-line blocked");
    assert_eq!(first.unwrap().value, 1);
    assert_eq!(second.unwrap().value, 2);
    runtime.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn global_call_admission_rejects_immediately() {
    let mut config = test_config();
    config.max_inflight_calls = 1;
    let runtime = LogicRuntime::new(
        config,
        memory_persistence(
            Arc::new(Mutex::new(HashMap::new())),
            Arc::new(Mutex::new(HashMap::new())),
        ),
    );
    let started = Arc::new(AtomicBool::new(false));
    let release = Arc::new(AtomicBool::new(false));
    let call_started = started.clone();
    let call_release = release.clone();
    let first = runtime
        .try_use(1, 1, move |_| {
            call_started.store(true, Ordering::Release);
            while !call_release.load(Ordering::Acquire) {
                std::thread::yield_now();
            }
        })
        .unwrap();
    wait_until(|| started.load(Ordering::Acquire)).await;

    assert!(matches!(
        runtime.try_use(2, 1, |_| ()),
        Err(RejectReason::Calls)
    ));
    release.store(true, Ordering::Release);
    first.await.unwrap();
    runtime.shutdown().await.unwrap();
    assert_eq!(runtime.stats().rejected_calls, 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn global_kib_and_per_gid_admission_are_bounded() {
    let mut kib_config = test_config();
    kib_config.max_inflight_kib = 1;
    let kib_runtime = LogicRuntime::new(
        kib_config,
        memory_persistence(
            Arc::new(Mutex::new(HashMap::new())),
            Arc::new(Mutex::new(HashMap::new())),
        ),
    );
    let started = Arc::new(AtomicBool::new(false));
    let release = Arc::new(AtomicBool::new(false));
    let call_started = started.clone();
    let call_release = release.clone();
    let first = kib_runtime
        .try_use(1, 1, move |_| {
            call_started.store(true, Ordering::Release);
            while !call_release.load(Ordering::Acquire) {
                std::thread::yield_now();
            }
        })
        .unwrap();
    wait_until(|| started.load(Ordering::Acquire)).await;
    assert!(matches!(
        kib_runtime.try_use(2, 1, |_| ()),
        Err(RejectReason::KiB)
    ));
    release.store(true, Ordering::Release);
    first.await.unwrap();
    kib_runtime.shutdown().await.unwrap();
    assert_eq!(kib_runtime.stats().rejected_kib, 1);

    let mut gid_config = test_config();
    gid_config.max_calls_per_gid = 1;
    let gid_runtime = LogicRuntime::new(
        gid_config,
        memory_persistence(
            Arc::new(Mutex::new(HashMap::new())),
            Arc::new(Mutex::new(HashMap::new())),
        ),
    );
    let started = Arc::new(AtomicBool::new(false));
    let release = Arc::new(AtomicBool::new(false));
    let call_started = started.clone();
    let call_release = release.clone();
    let first = gid_runtime
        .try_use(1, 1, move |_| {
            call_started.store(true, Ordering::Release);
            while !call_release.load(Ordering::Acquire) {
                std::thread::yield_now();
            }
        })
        .unwrap();
    wait_until(|| started.load(Ordering::Acquire)).await;
    assert!(matches!(
        gid_runtime.try_use(1, 1, |_| ()),
        Err(RejectReason::Gid)
    ));
    release.store(true, Ordering::Release);
    first.await.unwrap();
    gid_runtime.shutdown().await.unwrap();
    assert_eq!(gid_runtime.stats().rejected_gid, 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dirty_player_capacity_rejects_before_mutation() {
    let mut config = test_config();
    config.max_dirty_players = 1;
    let allow_save = Arc::new(AtomicBool::new(false));
    let saver_allow = allow_save.clone();
    let persistence = Persistence::new(
        |_| async { Ok::<_, TestError>(Player::default()) },
        move |player| {
            let allow = saver_allow.clone();
            async move {
                if !allow.load(Ordering::Acquire) {
                    return Err(TestError("save failed"));
                }
                player.with_mut(|state| state.dirty = false);
                Ok(())
            }
        },
    );
    let runtime = LogicRuntime::new(config, persistence);

    let first = runtime
        .try_use(1, 1, |player| player.dirty = true)
        .unwrap()
        .await
        .unwrap();
    assert!(first.persistence.is_err());
    wait_until(|| runtime.stats().active_gids == 0).await;

    let second_ran = Arc::new(AtomicBool::new(false));
    let observed = second_ran.clone();
    let error = runtime
        .try_use(2, 1, move |player| {
            observed.store(true, Ordering::Release);
            player.dirty = true;
        })
        .unwrap()
        .await
        .unwrap_err();
    assert!(matches!(error, LogicCallError::DirtyCapacity));
    assert!(!second_ran.load(Ordering::Acquire));
    let stats = runtime.stats();
    assert_eq!(stats.dirty_players, 1);
    assert_eq!(stats.dirty_slots, 1);
    assert_eq!(stats.dirty_slots_high_water, 1);
    assert_eq!(stats.rejected_dirty, 1);

    allow_save.store(true, Ordering::Release);
    runtime.shutdown().await.unwrap();
    assert_eq!(runtime.stats().dirty_players, 0);
    assert_eq!(runtime.stats().dirty_slots, 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn preload_is_async_outside_the_player_lock_but_inside_gid_serialization() {
    let runtime = LogicRuntime::new(
        test_config(),
        memory_persistence(
            Arc::new(Mutex::new(HashMap::new())),
            Arc::new(Mutex::new(HashMap::new())),
        ),
    );
    let preload_started = Arc::new(AtomicBool::new(false));
    let release_preload = Arc::new(Semaphore::new(0));
    let started = preload_started.clone();
    let release = release_preload.clone();
    let first = runtime
        .try_use_preloaded(
            1,
            1,
            move |player| {
                assert_eq!(player.value, 0);
                started.store(true, Ordering::Release);
                async move {
                    release.acquire().await.unwrap().forget();
                    Ok::<_, TestError>(41)
                }
            },
            |player, loaded| {
                player.value = loaded + 1;
                player.value
            },
        )
        .unwrap();
    wait_until(|| preload_started.load(Ordering::Acquire)).await;

    let second_ran = Arc::new(AtomicBool::new(false));
    let observed = second_ran.clone();
    let second = runtime
        .try_use(1, 1, move |_| {
            observed.store(true, Ordering::Release);
        })
        .unwrap();
    tokio::task::yield_now().await;
    assert!(!second_ran.load(Ordering::Acquire));

    let other = runtime.try_use(2, 1, |_| 7).unwrap();
    assert_eq!(
        timeout(Duration::from_secs(1), other)
            .await
            .unwrap()
            .unwrap()
            .value,
        7
    );
    release_preload.add_permits(1);
    assert_eq!(first.await.unwrap().value, 42);
    second.await.unwrap();
    assert!(second_ran.load(Ordering::Acquire));
    runtime.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn preload_failure_skips_business_logic() {
    let runtime = LogicRuntime::new(
        test_config(),
        memory_persistence(
            Arc::new(Mutex::new(HashMap::new())),
            Arc::new(Mutex::new(HashMap::new())),
        ),
    );
    let logic_ran = Arc::new(AtomicBool::new(false));
    let observed = logic_ran.clone();
    let error = runtime
        .try_use_preloaded(
            1,
            1,
            |_| async { Err::<(), _>(TestError("preload failed")) },
            move |_, ()| observed.store(true, Ordering::Release),
        )
        .unwrap()
        .await
        .unwrap_err();
    assert!(matches!(error, LogicCallError::Preload(_)));
    assert!(!logic_ran.load(Ordering::Acquire));
    let stats = runtime.stats();
    assert_eq!(stats.preload_calls, 1);
    assert_eq!(stats.preload_failed, 1);
    runtime.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dropping_the_call_does_not_cancel_accepted_logic() {
    let stored = Arc::new(Mutex::new(HashMap::new()));
    let runtime = LogicRuntime::new(
        test_config(),
        memory_persistence(stored.clone(), Arc::new(Mutex::new(HashMap::new()))),
    );
    let executed = Arc::new(AtomicBool::new(false));
    let observed = executed.clone();
    let call = runtime
        .try_use(9, 1, move |player| {
            player.value = 99;
            player.dirty = true;
            observed.store(true, Ordering::Release);
        })
        .unwrap();
    drop(call);

    runtime.shutdown().await.unwrap();
    assert!(executed.load(Ordering::Acquire));
    assert_eq!(stored.lock().unwrap().get(&9), Some(&99));
    assert_eq!(runtime.stats().completed, 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn capacity_one_keeps_one_active_generation_and_all_writes() {
    let mut config = test_config();
    config.resident_capacity = 1;
    config.shards = 1;
    let stored = Arc::new(Mutex::new(HashMap::new()));
    let loads = Arc::new(Mutex::new(HashMap::new()));
    let runtime = LogicRuntime::new(config, memory_persistence(stored.clone(), loads.clone()));
    let started = Arc::new(AtomicBool::new(false));
    let release = Arc::new(AtomicBool::new(false));
    let call_started = started.clone();
    let call_release = release.clone();
    let first = runtime
        .try_use(1, 1, move |player| {
            assert_eq!(player.generation, 1);
            player.value += 1;
            player.dirty = true;
            call_started.store(true, Ordering::Release);
            while !call_release.load(Ordering::Acquire) {
                std::thread::yield_now();
            }
        })
        .unwrap();
    wait_until(|| started.load(Ordering::Acquire)).await;

    let second = runtime
        .try_use(1, 1, |player| {
            assert_eq!(player.generation, 1);
            player.value += 1;
            player.dirty = true;
        })
        .unwrap();
    let other = runtime.try_use(2, 1, |_| ()).unwrap();
    release.store(true, Ordering::Release);
    timeout(Duration::from_secs(2), async {
        first.await.unwrap();
        second.await.unwrap();
        other.await.unwrap();
    })
    .await
    .unwrap();

    runtime.shutdown().await.unwrap();
    assert_eq!(loads.lock().unwrap().get(&1), Some(&1));
    assert_eq!(stored.lock().unwrap().get(&1), Some(&2));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn save_gate_covers_logic_and_save_without_holding_player_mutex() {
    let save_started = Arc::new(AtomicBool::new(false));
    let release_save = Arc::new(Semaphore::new(0));
    let first_save = Arc::new(AtomicBool::new(true));
    let saver_started = save_started.clone();
    let saver_release = release_save.clone();
    let saver_first = first_save.clone();
    let persistence = Persistence::new(
        |_| async { Ok::<_, TestError>(Player::default()) },
        move |player| {
            let started = saver_started.clone();
            let release = saver_release.clone();
            let should_wait = saver_first.swap(false, Ordering::AcqRel);
            async move {
                let _snapshot = player.with(|state| state.value);
                if should_wait {
                    started.store(true, Ordering::Release);
                    release.acquire().await.unwrap().forget();
                }
                player.with_mut(|state| state.dirty = false);
                Ok(())
            }
        },
    );
    let runtime = LogicRuntime::new(test_config(), persistence);
    let first = runtime
        .try_use(1, 1, |player| {
            player.value += 1;
            player.dirty = true;
        })
        .unwrap();
    wait_until(|| save_started.load(Ordering::Acquire)).await;

    let second_ran = Arc::new(AtomicBool::new(false));
    let observed = second_ran.clone();
    let second = runtime
        .try_use(1, 1, move |_| {
            observed.store(true, Ordering::Release);
        })
        .unwrap();
    tokio::task::yield_now().await;
    assert!(!second_ran.load(Ordering::Acquire));

    let other = runtime.try_use(2, 1, |_| 7).unwrap();
    assert_eq!(
        timeout(Duration::from_secs(1), other)
            .await
            .unwrap()
            .unwrap()
            .value,
        7
    );
    release_save.add_permits(1);
    first.await.unwrap();
    second.await.unwrap();
    assert!(second_ran.load(Ordering::Acquire));
    runtime.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn save_failure_is_separate_from_the_business_value() {
    let attempts = Arc::new(AtomicUsize::new(0));
    let saver_attempts = attempts.clone();
    let persistence = Persistence::new(
        |_| async { Ok::<_, TestError>(Player::default()) },
        move |player| {
            let attempts = saver_attempts.clone();
            async move {
                if attempts.fetch_add(1, Ordering::AcqRel) == 0 {
                    return Err(TestError("save failed"));
                }
                player.with_mut(|state| state.dirty = false);
                Ok(())
            }
        },
    );
    let runtime = LogicRuntime::new(test_config(), persistence);
    let completed = runtime
        .try_use(1, 1, |player| {
            player.dirty = true;
            42
        })
        .unwrap()
        .await
        .unwrap();
    assert_eq!(completed.value, 42);
    assert_eq!(completed.persistence.unwrap_err().0, "save failed");
    assert_eq!(runtime.stats().dirty_players, 1);
    wait_until(|| runtime.stats().active_gids == 0).await;
    assert_eq!(attempts.load(Ordering::Acquire), 1);

    runtime.shutdown().await.unwrap();
    assert_eq!(attempts.load(Ordering::Acquire), 2);
    assert_eq!(runtime.stats().dirty_players, 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn xmongo_prepared_save_retries_an_unacknowledged_failure() {
    let client = Client::with_options(ClientOptions::default()).unwrap();
    let collection = client.collection::<Document>("xkk_logic_unit_tests", "players");
    let load_collection = collection.clone();
    let attempts = Arc::new(AtomicUsize::new(0));
    let save_attempts = attempts.clone();
    let persistence = Persistence::new(
        move |gid| {
            let collection = load_collection.clone();
            async move {
                let mut data =
                    DataPersister::new(MongoPlayerData::default(), "playerData", collection, gid);
                data.set_loaded();
                Ok::<_, TestError>(MongoPlayer { data })
            }
        },
        move |player| {
            let prepared = player.with(|state| {
                state
                    .data
                    .prepare_save()
                    .unwrap()
                    .expect("dirty player must produce a prepared save")
            });
            let attempts = save_attempts.clone();
            async move {
                tokio::task::yield_now().await;
                if attempts.fetch_add(1, Ordering::AcqRel) == 0 {
                    return Err(TestError("save cancelled before acknowledgement"));
                }

                let acknowledgements = prepared.acknowledgements().cloned().collect::<Vec<_>>();
                player.with_mut(|state| {
                    for acknowledgement in &acknowledgements {
                        assert!(state.data.acknowledge(acknowledgement));
                    }
                });
                Ok(())
            }
        },
    );
    let runtime = LogicRuntime::new(test_config(), persistence);

    let completed = runtime
        .try_use(1, 1, |player| {
            player.data.raw_data_mut().value = 7;
            player.data.add_update_op("value").unwrap();
        })
        .unwrap()
        .await
        .unwrap();

    assert!(completed.persistence.is_err());
    assert_eq!(runtime.stats().dirty_players, 1);
    runtime.shutdown().await.unwrap();
    assert_eq!(attempts.load(Ordering::Acquire), 2);
    assert_eq!(runtime.stats().dirty_players, 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn load_failure_completes_the_accepted_call() {
    let persistence = Persistence::new(
        |_| async { Err::<Player, _>(TestError("load failed")) },
        |_| async { Ok::<_, TestError>(()) },
    );
    let runtime = LogicRuntime::new(test_config(), persistence);
    let error = runtime.try_use(1, 1, |_| ()).unwrap().await.unwrap_err();
    assert!(matches!(error, LogicCallError::Load(_)));
    runtime.shutdown().await.unwrap();
    let stats = runtime.stats();
    assert_eq!(stats.load_failed, 1);
    assert_eq!(stats.accepted, 1);
    assert_eq!(stats.completed, 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn shutdown_waits_for_the_same_drain_without_an_internal_timeout() {
    let runtime = LogicRuntime::new(
        test_config(),
        memory_persistence(
            Arc::new(Mutex::new(HashMap::new())),
            Arc::new(Mutex::new(HashMap::new())),
        ),
    );
    let started = Arc::new(AtomicBool::new(false));
    let release = Arc::new(AtomicBool::new(false));
    let call_started = started.clone();
    let call_release = release.clone();
    let call = runtime
        .try_use(1, 1, move |_| {
            call_started.store(true, Ordering::Release);
            while !call_release.load(Ordering::Acquire) {
                std::thread::yield_now();
            }
        })
        .unwrap();
    wait_until(|| started.load(Ordering::Acquire)).await;

    let shutdown = runtime.shutdown();
    tokio::pin!(shutdown);
    assert!(
        timeout(Duration::from_millis(20), &mut shutdown)
            .await
            .is_err()
    );
    assert_eq!(runtime.state(), RuntimeState::Draining);
    assert!(matches!(
        runtime.try_use(2, 1, |_| ()),
        Err(RejectReason::Draining)
    ));

    release.store(true, Ordering::Release);
    call.await.unwrap();
    shutdown.await.unwrap();
    assert_eq!(runtime.state(), RuntimeState::Stopped);
    assert_eq!(runtime.stats().rejected_draining, 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn shutdown_save_failure_remains_retryable() {
    let attempts = Arc::new(AtomicUsize::new(0));
    let saver_attempts = attempts.clone();
    let persistence = Persistence::new(
        |_| async { Ok::<_, TestError>(Player::default()) },
        move |player| {
            let attempts = saver_attempts.clone();
            async move {
                let attempt = attempts.fetch_add(1, Ordering::AcqRel);
                if attempt < 2 {
                    return Err(TestError("save failed"));
                }
                player.with_mut(|state| state.dirty = false);
                Ok(())
            }
        },
    );
    let runtime = LogicRuntime::new(test_config(), persistence);
    let completed = runtime
        .try_use(1, 1, |player| player.dirty = true)
        .unwrap()
        .await
        .unwrap();
    assert!(completed.persistence.is_err());
    wait_until(|| runtime.stats().active_gids == 0).await;
    assert_eq!(attempts.load(Ordering::Acquire), 1);

    assert!(matches!(
        runtime.shutdown().await,
        Err(ShutdownError::Persistence(_))
    ));
    assert_eq!(attempts.load(Ordering::Acquire), 2);
    assert_eq!(runtime.state(), RuntimeState::Draining);
    runtime.shutdown().await.unwrap();
    assert_eq!(runtime.state(), RuntimeState::Stopped);
    assert_eq!(attempts.load(Ordering::Acquire), 3);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn shutdown_uses_the_optional_batch_saver() {
    let single_calls = Arc::new(AtomicUsize::new(0));
    let batch_calls = Arc::new(AtomicUsize::new(0));
    let observed_single = single_calls.clone();
    let observed_batch = batch_calls.clone();
    let persistence = Persistence::new(
        |_| async { Ok::<_, TestError>(Player::default()) },
        move |_| {
            observed_single.fetch_add(1, Ordering::Relaxed);
            async { Err::<(), _>(TestError("single failed")) }
        },
    )
    .with_batch_saver(move |players| {
        let observed = observed_batch.clone();
        async move {
            observed.fetch_add(1, Ordering::Relaxed);
            for player in players {
                player.with_mut(|state| state.dirty = false);
            }
            Ok(())
        }
    });
    let runtime = LogicRuntime::new(test_config(), persistence);
    let completed = runtime
        .try_use(1, 1, |player| player.dirty = true)
        .unwrap()
        .await
        .unwrap();
    assert!(completed.persistence.is_err());

    runtime.shutdown().await.unwrap();
    assert_eq!(single_calls.load(Ordering::Relaxed), 1);
    assert_eq!(batch_calls.load(Ordering::Relaxed), 1);
    let stats = runtime.stats();
    assert_eq!(stats.dirty_players, 0);
    assert_eq!(stats.flush_latency.count(), 1);
}
