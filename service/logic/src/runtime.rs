use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use tokio::runtime::Handle;
use tokio::sync::{Mutex as AsyncMutex, Notify, oneshot};
use tokio::time::{Instant as TokioInstant, timeout_at};
use xlru::{CacheValue, Error as CacheError, Options, XlruCache};

use crate::persistence::{BoxFuture, LogicState, Persistence, SavePlayer};
use crate::stats::{LogicStats, StatsInner, update_high_water};

const ADMISSION_CLOSED: u64 = 1 << 63;
const ADMISSION_COUNT_MASK: u64 = ADMISSION_CLOSED - 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum RuntimeState {
    Running = 0,
    Draining = 1,
    Stopped = 2,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RejectReason {
    Calls,
    KiB,
    Gid,
    Draining,
}

#[derive(Debug)]
pub enum LogicCallError<E> {
    Load(Arc<E>),
    Preload(Arc<E>),
    Persistence(Arc<E>),
    DirtyCapacity,
    RuntimeStopped,
}

impl<E: fmt::Display> fmt::Display for LogicCallError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Load(error) => write!(f, "logic player load failed: {error}"),
            Self::Preload(error) => write!(f, "logic player preload failed: {error}"),
            Self::Persistence(error) => write!(f, "logic player persistence failed: {error}"),
            Self::DirtyCapacity => f.write_str("logic dirty player capacity exhausted"),
            Self::RuntimeStopped => f.write_str("logic runtime stopped before call completion"),
        }
    }
}

impl<E> std::error::Error for LogicCallError<E> where E: std::error::Error + 'static {}

#[derive(Debug)]
pub enum ShutdownError<E> {
    Timeout,
    Persistence(Arc<E>),
    DirtyPlayers(u64),
}

impl<E: fmt::Display> fmt::Display for ShutdownError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Timeout => f.write_str("logic runtime shutdown timed out"),
            Self::Persistence(error) => write!(f, "logic runtime flush failed: {error}"),
            Self::DirtyPlayers(count) => {
                write!(f, "logic runtime flush left {count} dirty players")
            }
        }
    }
}

impl<E> std::error::Error for ShutdownError<E> where E: std::error::Error + 'static {}

#[derive(Debug)]
pub struct Completed<R, E> {
    pub value: R,
    pub persistence: Result<(), Arc<E>>,
}

pub struct LogicCall<R, E> {
    receiver: oneshot::Receiver<Result<Completed<R, E>, LogicCallError<E>>>,
}

impl<R, E> Unpin for LogicCall<R, E> {}

impl<R, E> Future for LogicCall<R, E> {
    type Output = Result<Completed<R, E>, LogicCallError<E>>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match Pin::new(&mut self.receiver).poll(cx) {
            Poll::Ready(Ok(result)) => Poll::Ready(result),
            Poll::Ready(Err(_)) => Poll::Ready(Err(LogicCallError::RuntimeStopped)),
            Poll::Pending => Poll::Pending,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LogicConfig {
    pub resident_capacity: usize,
    pub ttl: Duration,
    pub shards: usize,
    pub batch_save_count: usize,
    pub max_dirty_players: usize,
    pub max_inflight_calls: usize,
    pub max_inflight_kib: usize,
    pub max_calls_per_gid: usize,
    pub max_kib_per_gid: usize,
}

impl Default for LogicConfig {
    fn default() -> Self {
        Self {
            resident_capacity: 100_000,
            ttl: Duration::from_secs(24 * 60 * 60),
            shards: 128,
            batch_save_count: 1_000,
            max_dirty_players: 100_000,
            max_inflight_calls: 100_000,
            max_inflight_kib: 256 * 1024,
            max_calls_per_gid: 64,
            max_kib_per_gid: 1_024,
        }
    }
}

impl LogicConfig {
    fn validate(&self) {
        assert!(
            self.resident_capacity > 0,
            "logic resident capacity must be greater than zero"
        );
        assert!(self.ttl > Duration::ZERO, "logic ttl must be non-zero");
        assert!(
            self.shards.is_power_of_two(),
            "logic shard count must be a non-zero power of two"
        );
        assert!(
            self.batch_save_count > 0,
            "logic batch save count must be greater than zero"
        );
        assert!(
            self.max_dirty_players > 0,
            "logic dirty player limit must be greater than zero"
        );
        assert!(
            self.max_inflight_calls > 0 && self.max_inflight_calls as u64 <= ADMISSION_COUNT_MASK,
            "logic inflight call limit is invalid"
        );
        assert!(
            self.max_inflight_kib > 0,
            "logic inflight KiB limit must be greater than zero"
        );
        assert!(
            self.max_calls_per_gid > 0,
            "logic per-gid call limit must be greater than zero"
        );
        assert!(
            self.max_kib_per_gid > 0,
            "logic per-gid KiB limit must be greater than zero"
        );
    }
}

pub struct LogicRuntime<P, E> {
    inner: Arc<RuntimeInner<P, E>>,
}

impl<P, E> Clone for LogicRuntime<P, E> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

struct PlayerCell<P> {
    id: u64,
    state: Arc<Mutex<P>>,
    save_gate: AsyncMutex<()>,
    dirty_slot: AtomicBool,
}

impl<P: LogicState> PlayerCell<P> {
    fn new(id: u64, state: P, stats: &StatsInner) -> Self {
        let dirty = state.is_dirty();
        assert!(!dirty, "logic loader returned a dirty player");
        let player = Self {
            id,
            state: Arc::new(Mutex::new(state)),
            save_gate: AsyncMutex::new(()),
            dirty_slot: AtomicBool::new(false),
        };
        stats.mark_dirty(id, dirty);
        player
    }

    fn dirty(&self) -> bool {
        self.state
            .lock()
            .expect("logic player mutex poisoned")
            .is_dirty()
    }

    fn finish_logic(
        &self,
        reservation: Option<DirtyPermit>,
        stats: &StatsInner,
        dirty_admission: &DirtyAdmission,
    ) -> bool {
        let dirty = self.dirty();
        if dirty && !self.has_dirty_slot() {
            let reservation = reservation.expect("logic dirty transition has no reservation");
            assert!(
                !self.dirty_slot.swap(true, Ordering::AcqRel),
                "logic clean player already owns a dirty slot"
            );
            reservation.commit();
        } else {
            assert!(
                reservation.is_none() || !dirty,
                "logic dirty player acquired a second dirty slot"
            );
        }
        self.refresh_dirty(stats, dirty_admission)
    }

    fn refresh_dirty(&self, stats: &StatsInner, dirty_admission: &DirtyAdmission) -> bool {
        let dirty = self.dirty();
        if dirty {
            assert!(
                self.dirty_slot.load(Ordering::Acquire),
                "logic dirty player has no dirty slot"
            );
        } else if self.dirty_slot.swap(false, Ordering::AcqRel) {
            dirty_admission.release();
        }
        stats.mark_dirty(self.id, dirty);
        dirty
    }

    fn has_dirty_slot(&self) -> bool {
        self.dirty_slot.load(Ordering::Acquire)
    }
}

impl<P: LogicState> CacheValue for PlayerCell<P> {
    fn is_dirty(&self) -> bool {
        self.dirty()
    }
}

struct DirectoryShard<P, E> {
    slots: Mutex<HashMap<i64, Arc<KeySlot<P, E>>>>,
}

struct KeySlot<P, E> {
    mailbox: Mutex<Mailbox<P, E>>,
}

struct Mailbox<P, E> {
    queue: VecDeque<Envelope<P, E>>,
    inflight_calls: usize,
    inflight_kib: usize,
}

struct Envelope<P, E> {
    command: Box<dyn Command<P, E>>,
    permit: GlobalPermit,
    retained_kib: usize,
    submitted_at: Instant,
}

trait Command<P, E>: Send {
    fn prepare(self: Box<Self>, state: &P) -> CommandPreparation<P, E>;
    fn fail(self: Box<Self>, error: LogicCallError<E>);
}

enum CommandPreparation<P, E> {
    Ready(Box<dyn ReadyCommand<P, E>>),
    Preloading(BoxFuture<PreloadOutcome<P, E>>),
}

enum PreloadOutcome<P, E> {
    Ready(Box<dyn ReadyCommand<P, E>>),
    Failed(Box<dyn FailedCommand>),
}

trait ReadyCommand<P, E>: Send {
    fn execute(self: Box<Self>, state: &mut P) -> Box<dyn Completion<E>>;
}

trait FailedCommand: Send {
    fn finish(self: Box<Self>);
}

trait Completion<E>: Send {
    fn finish(self: Box<Self>, persistence: Result<(), Arc<E>>);
}

struct TypedCommand<F, R, E> {
    logic: F,
    sender: oneshot::Sender<Result<Completed<R, E>, LogicCallError<E>>>,
}

impl<P, E, F, R> Command<P, E> for TypedCommand<F, R, E>
where
    F: FnOnce(&mut P) -> R + Send + 'static,
    R: Send + 'static,
    E: Send + Sync + 'static,
{
    fn prepare(self: Box<Self>, _state: &P) -> CommandPreparation<P, E> {
        CommandPreparation::Ready(self)
    }

    fn fail(self: Box<Self>, error: LogicCallError<E>) {
        let _ = self.sender.send(Err(error));
    }
}

impl<P, E, F, R> ReadyCommand<P, E> for TypedCommand<F, R, E>
where
    F: FnOnce(&mut P) -> R + Send + 'static,
    R: Send + 'static,
    E: Send + Sync + 'static,
{
    fn execute(self: Box<Self>, state: &mut P) -> Box<dyn Completion<E>> {
        let Self { logic, sender } = *self;
        Box::new(TypedCompletion {
            value: logic(state),
            sender,
        })
    }
}

type Preloader<P, A, E> = dyn FnOnce(&P) -> BoxFuture<Result<A, E>> + Send;

struct TypedPreloadedCommand<P, A, F, R, E> {
    preload: Box<Preloader<P, A, E>>,
    logic: F,
    sender: oneshot::Sender<Result<Completed<R, E>, LogicCallError<E>>>,
}

impl<P, A, F, R, E> Command<P, E> for TypedPreloadedCommand<P, A, F, R, E>
where
    P: Send + 'static,
    A: Send + 'static,
    F: FnOnce(&mut P, A) -> R + Send + 'static,
    R: Send + 'static,
    E: Send + Sync + 'static,
{
    fn prepare(self: Box<Self>, state: &P) -> CommandPreparation<P, E> {
        let Self {
            preload,
            logic,
            sender,
        } = *self;
        let preload = preload(state);
        CommandPreparation::Preloading(Box::pin(async move {
            match preload.await {
                Ok(value) => PreloadOutcome::Ready(Box::new(TypedPreparedCommand {
                    value,
                    logic,
                    sender,
                })
                    as Box<dyn ReadyCommand<P, E>>),
                Err(error) => PreloadOutcome::Failed(Box::new(TypedFailedCommand {
                    error: LogicCallError::Preload(Arc::new(error)),
                    sender,
                })),
            }
        }))
    }

    fn fail(self: Box<Self>, error: LogicCallError<E>) {
        let _ = self.sender.send(Err(error));
    }
}

struct TypedPreparedCommand<A, F, R, E> {
    value: A,
    logic: F,
    sender: oneshot::Sender<Result<Completed<R, E>, LogicCallError<E>>>,
}

impl<P, A, F, R, E> ReadyCommand<P, E> for TypedPreparedCommand<A, F, R, E>
where
    A: Send + 'static,
    F: FnOnce(&mut P, A) -> R + Send + 'static,
    R: Send + 'static,
    E: Send + Sync + 'static,
{
    fn execute(self: Box<Self>, state: &mut P) -> Box<dyn Completion<E>> {
        let Self {
            value,
            logic,
            sender,
        } = *self;
        Box::new(TypedCompletion {
            value: logic(state, value),
            sender,
        })
    }
}

struct TypedFailedCommand<R, E> {
    error: LogicCallError<E>,
    sender: oneshot::Sender<Result<Completed<R, E>, LogicCallError<E>>>,
}

impl<R, E> FailedCommand for TypedFailedCommand<R, E>
where
    R: Send + 'static,
    E: Send + Sync + 'static,
{
    fn finish(self: Box<Self>) {
        let _ = self.sender.send(Err(self.error));
    }
}

struct TypedCompletion<R, E> {
    value: R,
    sender: oneshot::Sender<Result<Completed<R, E>, LogicCallError<E>>>,
}

impl<R, E> Completion<E> for TypedCompletion<R, E>
where
    R: Send + 'static,
    E: Send + Sync + 'static,
{
    fn finish(self: Box<Self>, persistence: Result<(), Arc<E>>) {
        let _ = self.sender.send(Ok(Completed {
            value: self.value,
            persistence,
        }));
    }
}

struct Admission {
    calls: AtomicU64,
    kib: AtomicU64,
    max_calls: u64,
    max_kib: u64,
}

impl Admission {
    fn new(config: &LogicConfig) -> Self {
        Self {
            calls: AtomicU64::new(0),
            kib: AtomicU64::new(0),
            max_calls: config.max_inflight_calls as u64,
            max_kib: config.max_inflight_kib as u64,
        }
    }

    fn acquire(
        self: &Arc<Self>,
        retained_kib: u64,
        stats: &StatsInner,
    ) -> Result<GlobalPermit, RejectReason> {
        loop {
            let current = self.calls.load(Ordering::Acquire);
            if current & ADMISSION_CLOSED != 0 {
                stats.rejected_draining.fetch_add(1, Ordering::Relaxed);
                return Err(RejectReason::Draining);
            }
            if current >= self.max_calls {
                stats.rejected_calls.fetch_add(1, Ordering::Relaxed);
                return Err(RejectReason::Calls);
            }
            if self
                .calls
                .compare_exchange_weak(current, current + 1, Ordering::AcqRel, Ordering::Relaxed)
                .is_ok()
            {
                break;
            }
        }

        let kib_result = self
            .kib
            .fetch_update(Ordering::AcqRel, Ordering::Relaxed, |current| {
                current
                    .checked_add(retained_kib)
                    .filter(|&next| next <= self.max_kib)
            });
        let previous_kib = match kib_result {
            Ok(previous) => previous,
            Err(_) => {
                self.calls.fetch_sub(1, Ordering::Release);
                stats.rejected_kib.fetch_add(1, Ordering::Relaxed);
                return Err(RejectReason::KiB);
            }
        };

        let calls = self.calls.load(Ordering::Relaxed) & ADMISSION_COUNT_MASK;
        update_high_water(&stats.inflight_calls_high_water, calls);
        update_high_water(&stats.inflight_kib_high_water, previous_kib + retained_kib);
        Ok(GlobalPermit {
            admission: self.clone(),
            retained_kib,
        })
    }

    fn close(&self) {
        self.calls.fetch_or(ADMISSION_CLOSED, Ordering::AcqRel);
    }

    fn inflight_calls(&self) -> u64 {
        self.calls.load(Ordering::Acquire) & ADMISSION_COUNT_MASK
    }
}

struct GlobalPermit {
    admission: Arc<Admission>,
    retained_kib: u64,
}

impl Drop for GlobalPermit {
    fn drop(&mut self) {
        self.admission.calls.fetch_sub(1, Ordering::Release);
        self.admission
            .kib
            .fetch_sub(self.retained_kib, Ordering::Release);
    }
}

struct DirtyAdmission {
    slots: AtomicU64,
    max: u64,
}

impl DirtyAdmission {
    fn new(max: usize) -> Self {
        Self {
            slots: AtomicU64::new(0),
            max: max as u64,
        }
    }

    fn acquire(self: &Arc<Self>, stats: &StatsInner) -> Option<DirtyPermit> {
        let previous = self
            .slots
            .fetch_update(Ordering::AcqRel, Ordering::Relaxed, |current| {
                (current < self.max).then_some(current + 1)
            })
            .ok()?;
        update_high_water(&stats.dirty_slots_high_water, previous + 1);
        Some(DirtyPermit {
            admission: self.clone(),
            active: true,
        })
    }

    fn release(&self) {
        let previous = self.slots.fetch_sub(1, Ordering::Release);
        assert!(previous > 0, "logic dirty slot count underflow");
    }

    fn slots(&self) -> u64 {
        self.slots.load(Ordering::Acquire)
    }
}

struct DirtyPermit {
    admission: Arc<DirtyAdmission>,
    active: bool,
}

impl DirtyPermit {
    fn commit(mut self) {
        self.active = false;
    }
}

impl Drop for DirtyPermit {
    fn drop(&mut self) {
        if self.active {
            self.admission.release();
        }
    }
}

struct RuntimeInner<P, E> {
    config: LogicConfig,
    state: AtomicU8,
    admission: Arc<Admission>,
    dirty_admission: Arc<DirtyAdmission>,
    directories: Box<[DirectoryShard<P, E>]>,
    directory_mask: usize,
    cache: XlruCache<i64, Arc<PlayerCell<P>>, E>,
    persistence: Persistence<P, E>,
    stats: Arc<StatsInner>,
    drain_notify: Notify,
    executor: Handle,
}

impl<P, E> LogicRuntime<P, E>
where
    P: LogicState,
    E: Send + Sync + 'static,
{
    pub fn new(config: LogicConfig, persistence: Persistence<P, E>) -> Self {
        config.validate();
        let executor = Handle::try_current().expect("LogicRuntime::new requires a Tokio runtime");
        let stats = Arc::new(StatsInner::new());
        let next_player_id = Arc::new(AtomicU64::new(1));
        let dirty_admission = Arc::new(DirtyAdmission::new(config.max_dirty_players));

        let load_persistence = persistence.clone();
        let load_stats = stats.clone();
        let load_player_id = next_player_id.clone();
        let save_persistence = persistence.clone();
        let save_stats = stats.clone();
        let save_dirty_admission = dirty_admission.clone();

        let mut options = Options::new()
            .with_ttl(config.ttl)
            .with_shards(config.shards)
            .with_batch_save_count(config.batch_save_count)
            .with_loader(move |gid| {
                let persistence = load_persistence.clone();
                let stats = load_stats.clone();
                let player_id = load_player_id.fetch_add(1, Ordering::Relaxed);
                async move {
                    stats.load_calls.fetch_add(1, Ordering::Relaxed);
                    let started = Instant::now();
                    let result = (persistence.loader)(gid).await;
                    stats.load_latency.record(started.elapsed());
                    match result {
                        Ok(state) => Ok(Arc::new(PlayerCell::new(player_id, state, &stats))),
                        Err(error) => {
                            stats.load_failed.fetch_add(1, Ordering::Relaxed);
                            Err(error)
                        }
                    }
                }
            })
            .with_saver(move |gid, player: Arc<PlayerCell<P>>| {
                let persistence = save_persistence.clone();
                let stats = save_stats.clone();
                let dirty_admission = save_dirty_admission.clone();
                async move {
                    let _save = player.save_gate.lock().await;
                    save_one(&persistence, &stats, &dirty_admission, gid, &player).await
                }
            });

        if let Some(batch_saver) = persistence.batch_saver.clone() {
            let batch_stats = stats.clone();
            let batch_dirty_admission = dirty_admission.clone();
            options = options.with_batch_saver(move |mut entries| {
                let batch_saver = batch_saver.clone();
                let stats = batch_stats.clone();
                let dirty_admission = batch_dirty_admission.clone();
                async move {
                    entries.sort_unstable_by_key(|(gid, _)| *gid);
                    let mut gates = Vec::with_capacity(entries.len());
                    for (_, player) in &entries {
                        gates.push(player.save_gate.lock().await);
                    }

                    let dirty = entries
                        .iter()
                        .filter(|(_, player)| player.dirty())
                        .map(|(gid, player)| (*gid, player.clone()))
                        .collect::<Vec<_>>();
                    if dirty.is_empty() {
                        return Ok(());
                    }

                    stats
                        .save_calls
                        .fetch_add(dirty.len() as u64, Ordering::Relaxed);
                    let started = Instant::now();
                    let players = dirty
                        .iter()
                        .map(|(gid, player)| SavePlayer::new(*gid, player.state.clone()))
                        .collect();
                    let result = batch_saver(players).await;
                    stats.save_latency.record(started.elapsed());
                    if result.is_err() {
                        stats
                            .save_failed
                            .fetch_add(dirty.len() as u64, Ordering::Relaxed);
                    }
                    for (_, player) in dirty {
                        player.refresh_dirty(&stats, &dirty_admission);
                    }
                    result
                }
            });
        }

        let cache = XlruCache::new(config.resident_capacity, options);
        let directories = (0..config.shards)
            .map(|_| DirectoryShard {
                slots: Mutex::new(HashMap::new()),
            })
            .collect::<Vec<_>>()
            .into_boxed_slice();
        let admission = Arc::new(Admission::new(&config));

        Self {
            inner: Arc::new(RuntimeInner {
                config,
                state: AtomicU8::new(RuntimeState::Running as u8),
                admission,
                dirty_admission,
                directories,
                directory_mask: config.shards - 1,
                cache,
                persistence,
                stats,
                drain_notify: Notify::new(),
                executor,
            }),
        }
    }

    pub fn try_use<F, R>(
        &self,
        gid: i64,
        retained_kib: usize,
        logic: F,
    ) -> Result<LogicCall<R, E>, RejectReason>
    where
        F: FnOnce(&mut P) -> R + Send + 'static,
        R: Send + 'static,
    {
        self.inner.try_use(gid, retained_kib.max(1), logic)
    }

    /// Builds a preload future from an immutable player view, releases the
    /// player mutex while awaiting it, then applies the result in the same gid
    /// execution domain. Storage reads and owner-epoch checks belong here.
    pub fn try_use_preloaded<A, PF, PFut, F, R>(
        &self,
        gid: i64,
        retained_kib: usize,
        preload: PF,
        logic: F,
    ) -> Result<LogicCall<R, E>, RejectReason>
    where
        A: Send + 'static,
        PF: FnOnce(&P) -> PFut + Send + 'static,
        PFut: Future<Output = Result<A, E>> + Send + 'static,
        F: FnOnce(&mut P, A) -> R + Send + 'static,
        R: Send + 'static,
    {
        self.inner
            .try_use_preloaded(gid, retained_kib.max(1), preload, logic)
    }

    pub fn state(&self) -> RuntimeState {
        self.inner.state()
    }

    pub fn stats(&self) -> LogicStats {
        self.inner.stats()
    }

    pub async fn shutdown(&self, timeout: Duration) -> Result<(), ShutdownError<E>> {
        self.inner.shutdown(timeout).await
    }
}

impl<P, E> RuntimeInner<P, E>
where
    P: LogicState,
    E: Send + Sync + 'static,
{
    fn try_use<F, R>(
        self: &Arc<Self>,
        gid: i64,
        retained_kib: usize,
        logic: F,
    ) -> Result<LogicCall<R, E>, RejectReason>
    where
        F: FnOnce(&mut P) -> R + Send + 'static,
        R: Send + 'static,
    {
        let (sender, receiver) = oneshot::channel();
        self.enqueue(gid, retained_kib, Box::new(TypedCommand { logic, sender }))?;
        Ok(LogicCall { receiver })
    }

    fn try_use_preloaded<A, PF, PFut, F, R>(
        self: &Arc<Self>,
        gid: i64,
        retained_kib: usize,
        preload: PF,
        logic: F,
    ) -> Result<LogicCall<R, E>, RejectReason>
    where
        A: Send + 'static,
        PF: FnOnce(&P) -> PFut + Send + 'static,
        PFut: Future<Output = Result<A, E>> + Send + 'static,
        F: FnOnce(&mut P, A) -> R + Send + 'static,
        R: Send + 'static,
    {
        let (sender, receiver) = oneshot::channel();
        let preload =
            Box::new(move |state: &P| -> BoxFuture<Result<A, E>> { Box::pin(preload(state)) });
        self.enqueue(
            gid,
            retained_kib,
            Box::new(TypedPreloadedCommand {
                preload,
                logic,
                sender,
            }),
        )?;
        Ok(LogicCall { receiver })
    }

    fn enqueue(
        self: &Arc<Self>,
        gid: i64,
        retained_kib: usize,
        command: Box<dyn Command<P, E>>,
    ) -> Result<(), RejectReason> {
        let shard = self.directory(gid);
        let mut directory = shard.slots.lock().expect("logic directory mutex poisoned");
        let mut spawn_runner = false;
        let slot = match directory.get(&gid) {
            Some(slot) => slot.clone(),
            None => {
                let slot = Arc::new(KeySlot {
                    mailbox: Mutex::new(Mailbox {
                        queue: VecDeque::new(),
                        inflight_calls: 0,
                        inflight_kib: 0,
                    }),
                });
                directory.insert(gid, slot.clone());
                spawn_runner = true;
                slot
            }
        };
        let mut mailbox = slot.mailbox.lock().expect("logic mailbox mutex poisoned");

        if mailbox.inflight_calls >= self.config.max_calls_per_gid
            || retained_kib > self.config.max_kib_per_gid - mailbox.inflight_kib
        {
            self.stats.rejected_gid.fetch_add(1, Ordering::Relaxed);
            if spawn_runner {
                directory.remove(&gid);
            }
            return Err(RejectReason::Gid);
        }

        let permit = match self.admission.acquire(retained_kib as u64, &self.stats) {
            Ok(permit) => permit,
            Err(error) => {
                if spawn_runner {
                    directory.remove(&gid);
                }
                return Err(error);
            }
        };
        mailbox.inflight_calls += 1;
        mailbox.inflight_kib += retained_kib;
        mailbox.queue.push_back(Envelope {
            command,
            permit,
            retained_kib,
            submitted_at: Instant::now(),
        });
        let queued = self.stats.queued.fetch_add(1, Ordering::Relaxed) + 1;
        update_high_water(&self.stats.queued_high_water, queued);
        self.stats.accepted.fetch_add(1, Ordering::Relaxed);

        if spawn_runner {
            let active = self.stats.active_gids.fetch_add(1, Ordering::Relaxed) + 1;
            update_high_water(&self.stats.active_gids_high_water, active);
        }
        drop(mailbox);
        drop(directory);

        if spawn_runner {
            let runtime = self.clone();
            self.executor.spawn(async move {
                runtime.run_gid(gid, slot).await;
            });
        }

        Ok(())
    }

    async fn run_gid(self: Arc<Self>, gid: i64, slot: Arc<KeySlot<P, E>>) {
        let mut player = None;
        let mut next = Some(self.take_next(gid, &slot));

        while let Some(envelope) = next {
            let Envelope {
                command,
                permit,
                retained_kib,
                submitted_at,
            } = envelope;
            self.stats.queue_latency.record(submitted_at.elapsed());
            let run_started = Instant::now();

            if player.is_none() {
                match self.cache.get_i64(gid).await {
                    Ok(loaded) => player = Some(loaded),
                    Err(error) => {
                        self.stats.run_latency.record(run_started.elapsed());
                        command.fail(map_cache_error(error));
                        self.stats.total_latency.record(submitted_at.elapsed());
                        self.stats.completed.fetch_add(1, Ordering::Relaxed);
                        next = self
                            .finish_envelope(gid, &slot, retained_kib, permit, None)
                            .await;
                        continue;
                    }
                }
            }

            let current = player.as_ref().expect("logic player must be loaded");
            let save = current.save_gate.lock().await;
            let dirty_reservation = if current.has_dirty_slot() {
                None
            } else {
                match self.dirty_admission.acquire(&self.stats) {
                    Some(reservation) => Some(reservation),
                    None => {
                        drop(save);
                        self.stats.rejected_dirty.fetch_add(1, Ordering::Relaxed);
                        command.fail(LogicCallError::DirtyCapacity);
                        self.stats.run_latency.record(run_started.elapsed());
                        self.stats.total_latency.record(submitted_at.elapsed());
                        self.stats.completed.fetch_add(1, Ordering::Relaxed);
                        next = self
                            .finish_envelope(gid, &slot, retained_kib, permit, player.as_ref())
                            .await;
                        continue;
                    }
                }
            };

            let preparation = {
                let state = current.state.lock().expect("logic player mutex poisoned");
                command.prepare(&state)
            };
            let command = match preparation {
                CommandPreparation::Ready(command) => command,
                CommandPreparation::Preloading(preload) => {
                    self.stats.preload_calls.fetch_add(1, Ordering::Relaxed);
                    let started = Instant::now();
                    let outcome = preload.await;
                    self.stats.preload_latency.record(started.elapsed());
                    match outcome {
                        PreloadOutcome::Ready(command) => command,
                        PreloadOutcome::Failed(failure) => {
                            drop(dirty_reservation);
                            drop(save);
                            self.stats.preload_failed.fetch_add(1, Ordering::Relaxed);
                            self.stats.run_latency.record(run_started.elapsed());
                            self.stats.total_latency.record(submitted_at.elapsed());
                            self.stats.completed.fetch_add(1, Ordering::Relaxed);
                            failure.finish();
                            next = self
                                .finish_envelope(gid, &slot, retained_kib, permit, player.as_ref())
                                .await;
                            continue;
                        }
                    }
                }
            };
            let completion = {
                let mut state = current.state.lock().expect("logic player mutex poisoned");
                command.execute(&mut state)
            };
            let dirty = current.finish_logic(dirty_reservation, &self.stats, &self.dirty_admission);
            self.stats.run_latency.record(run_started.elapsed());
            let persistence = if dirty {
                save_one(
                    &self.persistence,
                    &self.stats,
                    &self.dirty_admission,
                    gid,
                    current,
                )
                .await
                .map_err(Arc::new)
            } else {
                Ok(())
            };
            drop(save);
            completion.finish(persistence);
            self.stats.total_latency.record(submitted_at.elapsed());
            self.stats.completed.fetch_add(1, Ordering::Relaxed);

            next = self
                .finish_envelope(gid, &slot, retained_kib, permit, player.as_ref())
                .await;
        }
    }

    fn take_next(&self, gid: i64, slot: &Arc<KeySlot<P, E>>) -> Envelope<P, E> {
        let directory = self
            .directory(gid)
            .slots
            .lock()
            .expect("logic directory mutex poisoned");
        debug_assert!(
            directory
                .get(&gid)
                .is_some_and(|current| Arc::ptr_eq(current, slot)),
            "logic runner lost its directory slot"
        );
        let mut mailbox = slot.mailbox.lock().expect("logic mailbox mutex poisoned");
        let next = mailbox
            .queue
            .pop_front()
            .expect("new logic runner must have one queued call");
        self.stats.queued.fetch_sub(1, Ordering::Relaxed);
        next
    }

    fn complete_and_take_next(
        &self,
        gid: i64,
        slot: &Arc<KeySlot<P, E>>,
        retained_kib: usize,
    ) -> Option<Envelope<P, E>> {
        let directory = self
            .directory(gid)
            .slots
            .lock()
            .expect("logic directory mutex poisoned");
        debug_assert!(
            directory
                .get(&gid)
                .is_some_and(|current| Arc::ptr_eq(current, slot)),
            "logic runner lost its directory slot"
        );
        let mut mailbox = slot.mailbox.lock().expect("logic mailbox mutex poisoned");
        mailbox.inflight_calls -= 1;
        mailbox.inflight_kib -= retained_kib;
        let next = mailbox.queue.pop_front();
        if next.is_some() {
            self.stats.queued.fetch_sub(1, Ordering::Relaxed);
        }
        next
    }

    async fn finish_envelope(
        &self,
        gid: i64,
        slot: &Arc<KeySlot<P, E>>,
        retained_kib: usize,
        permit: GlobalPermit,
        player: Option<&Arc<PlayerCell<P>>>,
    ) -> Option<Envelope<P, E>> {
        let mut next = self.complete_and_take_next(gid, slot, retained_kib);
        drop(permit);
        self.drain_notify.notify_waiters();
        if next.is_none() {
            next = self.retire_or_take_next(gid, slot, player).await;
        }
        next
    }

    async fn retire_or_take_next(
        &self,
        gid: i64,
        slot: &Arc<KeySlot<P, E>>,
        player: Option<&Arc<PlayerCell<P>>>,
    ) -> Option<Envelope<P, E>> {
        if let Some(player) = player {
            match self.cache.peek(&gid) {
                Some(resident) => {
                    assert!(
                        Arc::ptr_eq(&resident, player),
                        "logic cache contains a different active player generation"
                    );
                }
                None => {
                    let _ = self.cache.set_i64(gid, player.clone()).await;
                }
            }
        }

        let mut directory = self
            .directory(gid)
            .slots
            .lock()
            .expect("logic directory mutex poisoned");
        let Some(current) = directory.get(&gid) else {
            unreachable!("logic runner directory slot disappeared before retirement");
        };
        debug_assert!(Arc::ptr_eq(current, slot));
        let mut mailbox = slot.mailbox.lock().expect("logic mailbox mutex poisoned");
        if let Some(next) = mailbox.queue.pop_front() {
            self.stats.queued.fetch_sub(1, Ordering::Relaxed);
            return Some(next);
        }
        debug_assert_eq!(mailbox.inflight_calls, 0);
        debug_assert_eq!(mailbox.inflight_kib, 0);
        drop(mailbox);
        directory.remove(&gid);
        self.stats.active_gids.fetch_sub(1, Ordering::Relaxed);
        self.drain_notify.notify_waiters();
        None
    }

    fn directory(&self, gid: i64) -> &DirectoryShard<P, E> {
        let hash = mix_i64(gid);
        &self.directories[(hash as usize) & self.directory_mask]
    }

    fn state(&self) -> RuntimeState {
        match self.state.load(Ordering::Acquire) {
            0 => RuntimeState::Running,
            1 => RuntimeState::Draining,
            2 => RuntimeState::Stopped,
            _ => unreachable!("invalid logic runtime state"),
        }
    }

    fn stats(&self) -> LogicStats {
        LogicStats {
            inflight_calls: self.admission.inflight_calls(),
            inflight_calls_high_water: self.stats.inflight_calls_high_water.load(Ordering::Relaxed),
            inflight_kib: self.admission.kib.load(Ordering::Relaxed),
            inflight_kib_high_water: self.stats.inflight_kib_high_water.load(Ordering::Relaxed),
            queued: self.stats.queued.load(Ordering::Relaxed),
            queued_high_water: self.stats.queued_high_water.load(Ordering::Relaxed),
            active_gids: self.stats.active_gids.load(Ordering::Relaxed),
            active_gids_high_water: self.stats.active_gids_high_water.load(Ordering::Relaxed),
            dirty_players: self.stats.dirty_players.load(Ordering::Relaxed),
            dirty_slots: self.dirty_admission.slots(),
            dirty_slots_high_water: self.stats.dirty_slots_high_water.load(Ordering::Relaxed),
            accepted: self.stats.accepted.load(Ordering::Relaxed),
            completed: self.stats.completed.load(Ordering::Relaxed),
            load_calls: self.stats.load_calls.load(Ordering::Relaxed),
            load_failed: self.stats.load_failed.load(Ordering::Relaxed),
            preload_calls: self.stats.preload_calls.load(Ordering::Relaxed),
            preload_failed: self.stats.preload_failed.load(Ordering::Relaxed),
            save_calls: self.stats.save_calls.load(Ordering::Relaxed),
            save_failed: self.stats.save_failed.load(Ordering::Relaxed),
            rejected_calls: self.stats.rejected_calls.load(Ordering::Relaxed),
            rejected_kib: self.stats.rejected_kib.load(Ordering::Relaxed),
            rejected_gid: self.stats.rejected_gid.load(Ordering::Relaxed),
            rejected_dirty: self.stats.rejected_dirty.load(Ordering::Relaxed),
            rejected_draining: self.stats.rejected_draining.load(Ordering::Relaxed),
            queue_latency: self.stats.queue_latency.snapshot(),
            load_latency: self.stats.load_latency.snapshot(),
            run_latency: self.stats.run_latency.snapshot(),
            preload_latency: self.stats.preload_latency.snapshot(),
            save_latency: self.stats.save_latency.snapshot(),
            total_latency: self.stats.total_latency.snapshot(),
            flush_latency: self.stats.flush_latency.snapshot(),
            oldest_dirty_age: self.stats.oldest_dirty_age(),
            cache: self.cache.stats(),
        }
    }

    async fn shutdown(&self, timeout: Duration) -> Result<(), ShutdownError<E>> {
        if self.state() == RuntimeState::Stopped {
            return Ok(());
        }
        self.admission.close();
        self.state
            .compare_exchange(
                RuntimeState::Running as u8,
                RuntimeState::Draining as u8,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .ok();

        let deadline = TokioInstant::now() + timeout;
        loop {
            let drained = self.admission.inflight_calls() == 0
                && self.stats.active_gids.load(Ordering::Acquire) == 0;
            if drained {
                break;
            }
            let notified = self.drain_notify.notified();
            let drained = self.admission.inflight_calls() == 0
                && self.stats.active_gids.load(Ordering::Acquire) == 0;
            if drained {
                break;
            }
            timeout_at(deadline, notified)
                .await
                .map_err(|_| ShutdownError::Timeout)?;
        }

        let flush_started = Instant::now();
        let flushed = timeout_at(deadline, self.cache.flush()).await;
        self.stats.flush_latency.record(flush_started.elapsed());
        match flushed {
            Err(_) => return Err(ShutdownError::Timeout),
            Ok(Err(CacheError::Save(error) | CacheError::Load(error))) => {
                return Err(ShutdownError::Persistence(error));
            }
            Ok(Err(CacheError::MissingLoader | CacheError::MissingSaver)) => {
                unreachable!("logic runtime cache persistence callbacks are always configured");
            }
            Ok(Ok(())) => {}
        }

        let dirty = self
            .stats
            .dirty_players
            .load(Ordering::Acquire)
            .max(self.dirty_admission.slots());
        if dirty != 0 {
            return Err(ShutdownError::DirtyPlayers(dirty));
        }
        self.state
            .store(RuntimeState::Stopped as u8, Ordering::Release);
        Ok(())
    }
}

async fn save_one<P, E>(
    persistence: &Persistence<P, E>,
    stats: &StatsInner,
    dirty_admission: &DirtyAdmission,
    gid: i64,
    player: &Arc<PlayerCell<P>>,
) -> Result<(), E>
where
    P: LogicState,
    E: Send + Sync + 'static,
{
    if !player.dirty() {
        return Ok(());
    }
    stats.save_calls.fetch_add(1, Ordering::Relaxed);
    let started = Instant::now();
    let result = (persistence.saver)(SavePlayer::new(gid, player.state.clone())).await;
    stats.save_latency.record(started.elapsed());
    if result.is_err() {
        stats.save_failed.fetch_add(1, Ordering::Relaxed);
    }
    player.refresh_dirty(stats, dirty_admission);
    result
}

fn map_cache_error<E>(error: CacheError<E>) -> LogicCallError<E> {
    match error {
        CacheError::Load(error) => LogicCallError::Load(error),
        CacheError::Save(error) => LogicCallError::Persistence(error),
        CacheError::MissingLoader | CacheError::MissingSaver => {
            unreachable!("logic runtime cache persistence callbacks are always configured")
        }
    }
}

#[inline(always)]
fn mix_i64(value: i64) -> u64 {
    let mut value = value as u64;
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}
