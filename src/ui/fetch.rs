//! A page's own value, read in the background and again after every run.

use std::any::Any;
use std::sync::Arc;

use super::{Ctx, Loaded};
use crate::script::Failure;

type Job<T> = Arc<dyn Fn() -> Result<T, Failure> + Send + Sync>;

struct Tagged<T> {
    tag: &'static str,
    result: Result<T, Failure>,
}

pub struct Fetch<T> {
    pub value: Loaded<T>,
    tag: &'static str,
    generation: Option<u64>,
    job: Job<T>,
}

impl<T: Send + 'static> Fetch<T> {
    pub fn new(
        tag: &'static str,
        job: impl Fn() -> Result<T, Failure> + Send + Sync + 'static,
    ) -> Fetch<T> {
        Fetch {
            value: Loaded::Loading,
            tag,
            generation: None,
            job: Arc::new(job),
        }
    }

    /// Starts a read if there was none since the last run. The shown value stays until it arrives.
    pub fn ensure(&mut self, ctx: &Ctx) {
        if self.generation == Some(ctx.generation) {
            return;
        }
        self.generation = Some(ctx.generation);
        let (job, tag) = (self.job.clone(), self.tag);
        ctx.spawn(move || Tagged { tag, result: job() });
    }

    /// Takes the result if it is this fetch's, and returns whether it was.
    pub fn receive(&mut self, data: &mut Option<Box<dyn Any + Send>>) -> bool {
        let Some(boxed) = data.take() else {
            return false;
        };
        match boxed.downcast::<Tagged<T>>() {
            Ok(tagged) if tagged.tag == self.tag => {
                self.value = Loaded::from(tagged.result);
                true
            }
            Ok(other) => {
                *data = Some(other);
                false
            }
            Err(other) => {
                *data = Some(other);
                false
            }
        }
    }
}
