//! A page's own values, read in the background: one read anew after every run, one awaited once,
//! each through a channel of its type. Beside them the one thread that writes.

use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

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

type Write = Box<dyn FnOnce() + Send>;

static SERIAL: OnceLock<Sender<Write>> = OnceLock::new();

/// Runs a job on the one thread for what has to happen in order, such as writing the settings
/// twice in a row: the disk never holds the interface.
pub fn serial(job: impl FnOnce() + Send + 'static) {
    let tx = SERIAL.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<Write>();
        std::thread::spawn(move || rx.into_iter().for_each(|job| job()));
        tx
    });
    let _ = tx.send(Box::new(job));
}

/// Waits for what `serial` was handed, as settings chosen right before quitting, at most `limit`.
pub fn settle(limit: Duration) -> bool {
    let Some(tx) = SERIAL.get() else {
        return true;
    };
    let (done, wait) = mpsc::channel();
    let _ = tx.send(Box::new(move || {
        let _ = done.send(());
    }));
    wait.recv_timeout(limit).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_run_in_order_and_the_end_waits_for_them() {
        let (tx, rx) = mpsc::channel();
        for n in 0..3 {
            let tx = tx.clone();
            serial(move || {
                std::thread::sleep(Duration::from_millis(20));
                tx.send(n).unwrap();
            });
        }
        assert!(settle(Duration::from_secs(5)));
        assert_eq!(rx.try_iter().collect::<Vec<_>>(), [0, 1, 2]);
        serial(|| std::thread::sleep(Duration::from_millis(200)));
        assert!(!settle(Duration::from_millis(20)), "gives up at the limit");
    }
}
