use std::convert::Infallible;
use std::env;
use std::hint::black_box;
use std::sync::Arc;
use std::time::Instant;

use xkk_logic::{LogicConfig, LogicRuntime, LogicState, Persistence};

const DEFAULT_OPS: usize = 1_000_000;
const DEFAULT_WORKERS: usize = 64;

#[derive(Default)]
struct Player {
    value: u64,
}

impl LogicState for Player {
    fn is_dirty(&self) -> bool {
        false
    }
}

fn main() {
    tokio::runtime::Builder::new_multi_thread().enable_time().build().unwrap().block_on(run());
}

async fn run() {
    let operations = env_usize("XKK_LOGIC_BENCH_OPS", DEFAULT_OPS);
    let workers = env_usize("XKK_LOGIC_BENCH_WORKERS", DEFAULT_WORKERS);
    assert!(operations >= workers, "operations must cover every worker");

    let config = LogicConfig {
        resident_capacity: workers,
        max_dirty_players: workers,
        max_inflight_calls: workers * 2,
        max_inflight_kib: workers * 2,
        max_calls_per_gid: 2,
        max_kib_per_gid: 2,
        ..LogicConfig::default()
    };
    let persistence = Persistence::new(|_| async { Ok::<_, Infallible>(Player::default()) }, |_| async { Ok::<_, Infallible>(()) });
    let runtime = Arc::new(LogicRuntime::new(config, persistence));

    for gid in 0..workers as i64 {
        runtime.try_use(gid, 1, |_| ()).unwrap().await.unwrap();
    }

    let started = Instant::now();
    let mut tasks = Vec::with_capacity(workers);
    for worker in 0..workers {
        let runtime = runtime.clone();
        let count = operations / workers + usize::from(worker < operations % workers);
        tasks.push(tokio::spawn(async move {
            for _ in 0..count {
                let completed = runtime
                    .try_use(worker as i64, 1, |player| {
                        player.value = player.value.wrapping_add(1);
                        black_box(player.value)
                    })
                    .unwrap()
                    .await
                    .unwrap();
                black_box(completed.value);
            }
        }));
    }
    for task in tasks {
        task.await.unwrap();
    }
    let elapsed = started.elapsed();
    let stats = runtime.stats();
    let p99 = stats.total_latency.percentile_micros(99.0).unwrap();
    let p999 = stats.total_latency.percentile_micros(99.9).unwrap();
    println!(
        "xkk_logic_runtime: operations={operations} workers={workers} elapsed_ms={:.3} ns_per_op={:.2} throughput_ops_per_sec={:.0} p99_upper_us={p99} p999_upper_us={p999}",
        elapsed.as_secs_f64() * 1_000.0,
        elapsed.as_secs_f64() * 1_000_000_000.0 / operations as f64,
        operations as f64 / elapsed.as_secs_f64(),
    );

    runtime.shutdown().await.unwrap();
    let final_stats = runtime.stats();
    assert_eq!(final_stats.inflight_calls, 0);
    assert_eq!(final_stats.queued, 0);
    assert_eq!(final_stats.active_gids, 0);
    assert_eq!(final_stats.dirty_players, 0);
    assert_eq!(final_stats.dirty_slots, 0);
}

fn env_usize(name: &str, default: usize) -> usize {
    env::var(name).ok().map(|value| value.parse().expect("benchmark environment value must be usize")).unwrap_or(default)
}
