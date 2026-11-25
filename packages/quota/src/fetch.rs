use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread::JoinHandle;
use std::time::Duration;

use proteus_extension::{Host, Result, Value, anyhow};

pub const POLL: Duration = Duration::from_millis(50);
const LIMIT: usize = 4;
static IN_FLIGHT: AtomicUsize = AtomicUsize::new(0);

struct Slot;

impl Slot {
    fn try_acquire() -> Option<Self> {
        IN_FLIGHT
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |count| {
                (count < LIMIT).then_some(count + 1)
            })
            .ok()
            .map(|_| Self)
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        IN_FLIGHT.fetch_sub(1, Ordering::Relaxed);
    }
}

/// Dropping the receiver makes obsolete responses ineligible for presentation.
/// The bounded worker may finish its HTTP call in parallel with the new model.
pub struct Pending {
    receiver: Receiver<Result<Value>>,
    worker: JoinHandle<()>,
}

impl Pending {
    pub fn is_ready(&self) -> bool {
        self.worker.is_finished()
    }

    pub fn result(&self) -> Option<Result<Value>> {
        match self.receiver.try_recv() {
            Ok(result) => Some(result),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(Err(anyhow!("quota request stopped"))),
        }
    }
}

pub fn start(host: &Host) -> Result<Option<Pending>> {
    let Some(slot) = Slot::try_acquire() else {
        return Ok(None);
    };
    let request = host.prepare_get("/model/quota")?;
    let (sender, receiver) = mpsc::sync_channel(1);
    let worker = std::thread::Builder::new()
        .name("quota-http".into())
        .spawn(move || {
            let result = request();
            drop(slot);
            let _ = sender.send(result);
        })?;
    Ok(Some(Pending { receiver, worker }))
}
