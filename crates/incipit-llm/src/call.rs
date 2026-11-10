//! Calls in the background: how they report, how they end and how they are stopped. A provider's
//! methods block; `Call::spawn` runs one on a thread of its own and hands back its updates (text as
//! it arrives, what an agent is doing, then the result) through a channel, or through a callback
//! with `spawn_with`, so the window's executor and a plain thread can both use it. No tokio.
//!
//! Cancelling is an event, not a flag someone checks: `Cancel::cancel` runs the hooks a call
//! registered (kill this process group, shut this connection), so a call stops at once even while
//! it waits on a silent child or a silent server.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex, Weak};
use std::thread;
use std::time::Duration;

use serde_json::Value;

use crate::error::Error;

/// Something a call reports before it ends.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    /// More of the answer's text.
    Text(String),
    /// What the model or agent is doing, in a few words: "reading notes.md".
    Activity(String),
    /// A page the learner should open (a sign-in page), in case the browser did not.
    Link(String),
}

/// The end of a call that answered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Answer {
    pub text: String,
    /// The provider's id ("codex", "anthropic").
    pub provider: String,
    /// The model that answered, as far as the provider says.
    pub model: String,
}

/// The end of a structured call: JSON that matches the schema.
#[derive(Clone, Debug, PartialEq)]
pub struct Structured {
    pub value: Value,
    pub answer: Answer,
    /// The first answer did not match and the model corrected it.
    pub repaired: bool,
}

type Hooks = Vec<(u64, Box<dyn FnOnce() + Send>)>;

#[derive(Default)]
struct Inner {
    cancelled: AtomicBool,
    next: AtomicU64,
    hooks: Mutex<Hooks>,
}

/// Stops a call. Clones stop the same call.
#[derive(Clone, Default)]
pub struct Cancel {
    inner: Arc<Inner>,
}

impl std::fmt::Debug for Cancel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Cancel")
            .field("cancelled", &self.is_cancelled())
            .finish()
    }
}

/// A registered cancel hook; dropping it unregisters the hook.
#[must_use = "the hook is removed when this is dropped"]
pub struct Hook {
    inner: Weak<Inner>,
    id: u64,
}

impl Drop for Hook {
    fn drop(&mut self) {
        if let Some(inner) = self.inner.upgrade() {
            lock(&inner.hooks).retain(|(id, _)| *id != self.id);
        }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    // A hook that panicked leaves nothing half-written behind: the list is still a list.
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl Cancel {
    pub fn new() -> Self {
        Cancel::default()
    }

    /// Stops the call: every hook runs once, now.
    pub fn cancel(&self) {
        if self.inner.cancelled.swap(true, Ordering::SeqCst) {
            return;
        }
        let hooks = std::mem::take(&mut *lock(&self.inner.hooks));
        for (_, hook) in hooks {
            hook();
        }
    }

    pub fn is_cancelled(&self) -> bool {
        self.inner.cancelled.load(Ordering::SeqCst)
    }

    /// Runs `hook` when the call is cancelled, or now if it already is.
    pub fn on_cancel(&self, hook: impl FnOnce() + Send + 'static) -> Hook {
        let id = self.inner.next.fetch_add(1, Ordering::SeqCst);
        let handle = Hook {
            inner: Arc::downgrade(&self.inner),
            id,
        };
        {
            let mut hooks = lock(&self.inner.hooks);
            if !self.is_cancelled() {
                hooks.push((id, Box::new(hook)));
                return handle;
            }
        }
        hook();
        handle
    }

    pub(crate) fn check(&self) -> Result<(), Error> {
        if self.is_cancelled() {
            Err(Error::Cancelled)
        } else {
            Ok(())
        }
    }

    /// Waits `time`, or less if the call is cancelled meanwhile.
    pub fn wait(&self, time: Duration) -> Result<(), Error> {
        let (tx, rx) = mpsc::channel::<()>();
        let _hook = self.on_cancel(move || {
            let _ = tx.send(());
        });
        match rx.recv_timeout(time) {
            Ok(()) => Err(Error::Cancelled),
            Err(RecvTimeoutError::Timeout) => self.check(),
            Err(RecvTimeoutError::Disconnected) => self.check(),
        }
    }
}

/// What a background call sends.
#[derive(Debug)]
pub enum Update<T> {
    Event(Event),
    Done(Result<T, Error>),
}

/// A call running on a thread of its own.
pub struct Call<T> {
    updates: Receiver<Update<T>>,
    cancel: Cancel,
}

impl<T: Send + 'static> Call<T> {
    /// Runs `work` on a new thread; its events and its result arrive on `updates()`.
    pub fn spawn<F>(work: F) -> Call<T>
    where
        F: FnOnce(&Cancel, &mut dyn FnMut(Event)) -> Result<T, Error> + Send + 'static,
    {
        let (tx, rx) = mpsc::channel();
        let cancel = spawn_with(work, move |update| {
            let _ = tx.send(update);
        });
        Call {
            updates: rx,
            cancel,
        }
    }

    /// The updates, in order; the last is `Update::Done`.
    pub fn updates(&self) -> &Receiver<Update<T>> {
        &self.updates
    }

    pub fn cancel(&self) {
        self.cancel.cancel();
    }

    /// A handle that stops this call from elsewhere.
    pub fn canceller(&self) -> Cancel {
        self.cancel.clone()
    }

    /// Blocks until the call ends, passing each event to `on`.
    pub fn wait_with(self, mut on: impl FnMut(Event)) -> Result<T, Error> {
        for update in self.updates.iter() {
            match update {
                Update::Event(event) => on(event),
                Update::Done(result) => return result,
            }
        }
        // The thread sends `Done` last unless the work panicked.
        Err(Error::provider(
            "Incipit",
            "the call stopped without an answer (an error inside Incipit)",
        ))
    }

    pub fn wait(self) -> Result<T, Error> {
        self.wait_with(|_| {})
    }
}

/// Runs `work` on a new thread and gives every update to `on_update`, on that thread. Returns
/// what stops it.
pub fn spawn_with<T, F, U>(work: F, mut on_update: U) -> Cancel
where
    T: Send + 'static,
    F: FnOnce(&Cancel, &mut dyn FnMut(Event)) -> Result<T, Error> + Send + 'static,
    U: FnMut(Update<T>) + Send + 'static,
{
    let cancel = Cancel::new();
    let theirs = cancel.clone();
    thread::spawn(move || {
        let result = {
            let mut events = |event: Event| on_update(Update::Event(event));
            let result = work(&theirs, &mut events);
            match result {
                Err(_) if theirs.is_cancelled() => Err(Error::Cancelled),
                other => other,
            }
        };
        on_update(Update::Done(result));
    });
    cancel
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[test]
    fn a_hook_runs_once_when_the_call_is_cancelled() {
        let cancel = Cancel::new();
        let (tx, rx) = mpsc::channel();
        let _hook = cancel.on_cancel(move || tx.send(()).unwrap());
        cancel.cancel();
        cancel.cancel();
        assert!(rx.recv().is_ok());
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn a_hook_added_after_cancelling_runs_at_once() {
        let cancel = Cancel::new();
        cancel.cancel();
        let (tx, rx) = mpsc::channel();
        let _hook = cancel.on_cancel(move || tx.send(()).unwrap());
        assert!(rx.try_recv().is_ok());
    }

    #[test]
    fn a_dropped_hook_does_not_run() {
        let cancel = Cancel::new();
        let (tx, rx) = mpsc::channel::<()>();
        drop(cancel.on_cancel(move || tx.send(()).unwrap()));
        cancel.cancel();
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn waiting_ends_as_soon_as_the_call_is_cancelled() {
        let cancel = Cancel::new();
        let theirs = cancel.clone();
        let started = Instant::now();
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(50));
            theirs.cancel();
        });
        assert!(matches!(
            cancel.wait(Duration::from_secs(30)),
            Err(Error::Cancelled)
        ));
        assert!(started.elapsed() < Duration::from_secs(10));
    }

    #[test]
    fn a_spawned_call_sends_its_events_then_its_result() {
        let call = Call::spawn(|_, on| {
            on(Event::Text("Hel".into()));
            on(Event::Text("lo".into()));
            Ok(42)
        });
        let mut text = String::new();
        let result = call.wait_with(|event| {
            if let Event::Text(t) = event {
                text.push_str(&t)
            }
        });
        assert_eq!(result.unwrap(), 42);
        assert_eq!(text, "Hello");
    }

    #[test]
    fn a_cancelled_call_ends_as_cancelled_whatever_its_work_said() {
        let call: Call<()> = Call::spawn(|cancel, _| {
            cancel.wait(Duration::from_secs(30))?;
            Err(Error::Unsupported("unreachable".into()))
        });
        call.cancel();
        assert!(matches!(call.wait(), Err(Error::Cancelled)));
    }
}
