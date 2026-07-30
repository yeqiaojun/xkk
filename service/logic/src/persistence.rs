use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

pub trait LogicState: Send + 'static {
    fn is_dirty(&self) -> bool;
}

pub struct SavePlayer<P> {
    gid: i64,
    state: Arc<Mutex<P>>,
}

impl<P> SavePlayer<P> {
    pub(crate) fn new(gid: i64, state: Arc<Mutex<P>>) -> Self {
        Self { gid, state }
    }

    pub fn gid(&self) -> i64 {
        self.gid
    }

    pub fn with<R>(&self, f: impl FnOnce(&P) -> R) -> R {
        let state = self.state.lock().expect("logic player mutex poisoned");
        f(&state)
    }

    pub fn with_mut<R>(&self, f: impl FnOnce(&mut P) -> R) -> R {
        let mut state = self.state.lock().expect("logic player mutex poisoned");
        f(&mut state)
    }
}

pub(crate) type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send + 'static>>;
type Loader<P, E> = dyn Fn(i64) -> BoxFuture<Result<P, E>> + Send + Sync;
type Saver<P, E> = dyn Fn(SavePlayer<P>) -> BoxFuture<Result<(), E>> + Send + Sync;
type BatchSaver<P, E> = dyn Fn(Vec<SavePlayer<P>>) -> BoxFuture<Result<(), E>> + Send + Sync;

pub struct Persistence<P, E> {
    pub(crate) loader: Arc<Loader<P, E>>,
    pub(crate) saver: Arc<Saver<P, E>>,
    pub(crate) batch_saver: Option<Arc<BatchSaver<P, E>>>,
}

impl<P, E> Clone for Persistence<P, E> {
    fn clone(&self) -> Self {
        Self {
            loader: self.loader.clone(),
            saver: self.saver.clone(),
            batch_saver: self.batch_saver.clone(),
        }
    }
}

impl<P, E> Persistence<P, E> {
    pub fn new<L, LF, S, SF>(loader: L, saver: S) -> Self
    where
        L: Fn(i64) -> LF + Send + Sync + 'static,
        LF: Future<Output = Result<P, E>> + Send + 'static,
        S: Fn(SavePlayer<P>) -> SF + Send + Sync + 'static,
        SF: Future<Output = Result<(), E>> + Send + 'static,
    {
        Self {
            loader: Arc::new(move |gid| Box::pin(loader(gid))),
            saver: Arc::new(move |player| Box::pin(saver(player))),
            batch_saver: None,
        }
    }

    pub fn with_batch_saver<B, BF>(mut self, saver: B) -> Self
    where
        B: Fn(Vec<SavePlayer<P>>) -> BF + Send + Sync + 'static,
        BF: Future<Output = Result<(), E>> + Send + 'static,
    {
        self.batch_saver = Some(Arc::new(move |players| Box::pin(saver(players))));
        self
    }
}
