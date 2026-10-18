//! A page's own values, read in the background: one read anew after every run, one awaited once.
//! Each comes back through a channel of its type, and the page is told when to look.

use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};

use super::{Ctx, Loaded};
use crate::script::Failure;

type Job<T> = Arc<dyn Fn() -> Result<T, Failure> + Send + Sync>;

/// A value read in the background, and again after every run.
pub struct Fetch<T> {
    pub value: Loaded<T>,
    generation: Option<u64>,
    job: Job<T>,
    tx: Sender<Result<T, Failure>>,
    rx: Receiver<Result<T, Failure>>,
}

impl<T: Send + 'static> Fetch<T> {
    pub fn new(job: impl Fn() -> Result<T, Failure> + Send + Sync + 'static) -> Fetch<T> {
        let (tx, rx) = mpsc::channel();
        Fetch {
            value: Loaded::Loading,
            generation: None,
            job: Arc::new(job),
            tx,
            rx,
        }
    }

    /// Starts a read if there was none since the last run. The shown value stays until it arrives.
    pub fn ensure(&mut self, ctx: &Ctx) {
        if self.generation == Some(ctx.generation) {
            return;
        }
        self.generation = Some(ctx.generation);
        let (job, tx) = (self.job.clone(), self.tx.clone());
        ctx.spawn(move || {
            let _ = tx.send(job());
        });
    }

    /// Takes what arrived, the newest where several did.
    pub fn receive(&mut self) {
        while let Ok(result) = self.rx.try_recv() {
            self.value = Loaded::from(result);
        }
    }
}

/// A result awaited once, from a job the page started.
pub struct Task<T> {
    rx: Receiver<T>,
}

impl<T: Send + 'static> Task<T> {
    pub fn start(ctx: &Ctx, job: impl FnOnce() -> T + Send + 'static) -> Task<T> {
        let (tx, rx) = mpsc::channel();
        ctx.spawn(move || {
            let _ = tx.send(job());
        });
        Task { rx }
    }

    /// One that never finishes, for a page shown as it waits.
    #[cfg(test)]
    pub fn pending() -> Task<T> {
        Task {
            rx: mpsc::channel().1,
        }
    }

    /// The result, once it has arrived.
    pub fn take(&mut self) -> Option<T> {
        self.rx.try_recv().ok()
    }
}
