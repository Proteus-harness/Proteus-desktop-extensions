//! Ordered stdin admission. Permits survive dequeue until the handler finishes,
//! so a blocked HTTP/action handler cannot drain the pipe into unbounded Values.
use std::io::{self, BufRead, Read};
use std::sync::{Arc, Condvar, Mutex, mpsc};

use serde_json::Value;

const FRAMES: usize = 128;
const BYTES: usize = 32 * 1024 * 1024;
const FRAME_BYTES: usize = 16 * 1024 * 1024;

#[derive(Default)]
struct State {
    frames: usize,
    bytes: usize,
}

struct Budget {
    state: Mutex<State>,
    ready: Condvar,
    frames: usize,
    bytes: usize,
}

struct Permit {
    budget: Arc<Budget>,
    bytes: usize,
}

impl Drop for Permit {
    fn drop(&mut self) {
        let mut state = self.budget.state.lock().unwrap();
        state.frames -= 1;
        state.bytes -= self.bytes;
        self.budget.ready.notify_all();
    }
}

pub(super) struct Incoming {
    pub value: Value,
    _permit: Permit,
}

impl Incoming {
    pub fn take(&mut self) -> Value {
        self.value.take()
    }
}

impl Budget {
    fn new(frames: usize, bytes: usize) -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(State::default()),
            ready: Condvar::new(),
            frames,
            bytes,
        })
    }

    fn acquire(self: &Arc<Self>, bytes: usize) -> Permit {
        assert!(bytes <= self.bytes);
        let mut state = self.state.lock().unwrap();
        while state.frames >= self.frames || state.bytes > self.bytes - bytes {
            state = self.ready.wait(state).unwrap();
        }
        state.frames += 1;
        state.bytes += bytes;
        Permit {
            budget: self.clone(),
            bytes,
        }
    }
}

/// One bounded frame may be read ahead of admitted messages. No parser or
/// unbounded `lines()` allocation runs before its encoded-byte reservation.
fn frame(reader: &mut impl BufRead, limit: usize) -> io::Result<Option<Vec<u8>>> {
    let mut bytes = Vec::new();
    reader
        .take((limit + 1) as u64)
        .read_until(b'\n', &mut bytes)?;
    if bytes.len() > limit {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "extension input frame exceeds 16 MiB",
        ));
    }
    Ok((!bytes.is_empty()).then_some(bytes))
}

pub(super) fn start() -> mpsc::Receiver<Incoming> {
    let (sender, inbox) = mpsc::sync_channel(FRAMES);
    std::thread::spawn(move || {
        let budget = Budget::new(FRAMES, BYTES);
        let mut stdin = std::io::stdin().lock();
        loop {
            let line = match frame(&mut stdin, FRAME_BYTES) {
                Ok(Some(line)) => line,
                Ok(None) => break,
                Err(error) => {
                    eprintln!("extension: {error}");
                    break;
                }
            };
            let permit = budget.acquire(line.len());
            match serde_json::from_slice(&line) {
                Ok(value) => {
                    if sender
                        .send(Incoming {
                            value,
                            _permit: permit,
                        })
                        .is_err()
                    {
                        break;
                    }
                }
                Err(error) => eprintln!("extension: bad message: {error}"),
            }
        }
    });
    inbox
}

/// Outstanding requests remain ordered until command_result. Reject new work
/// at admission instead of retaining arbitrary values while the backend stalls.
pub(super) fn settings_budget<'a>(
    pending: impl IntoIterator<Item = &'a [(String, String, Value)]>,
    kind: &str,
    value: &Value,
) -> io::Result<()> {
    struct Counter(usize);
    impl io::Write for Counter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0 = self
                .0
                .checked_add(bytes.len())
                .filter(|size| *size <= 1024 * 1024)
                .ok_or_else(|| io::Error::other("pending session settings exceed 1 MiB"))?;
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    // Reserve the next generated id and tuple punctuation as well as values.
    let mut counter = Counter(64);
    let mut count = 0usize;
    for session in pending {
        count = count.saturating_add(session.len());
        if count >= 128 {
            return Err(io::Error::other("too many pending session settings"));
        }
        serde_json::to_writer(&mut counter, session)?;
    }
    serde_json::to_writer(&mut counter, &(kind, value))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn a_slow_handler_retains_admission_and_resumes_in_order() {
        let budget = Budget::new(2, 6);
        let first = budget.acquire(3);
        let second = budget.acquire(3);
        let (tx, rx) = mpsc::channel();
        let producer = std::thread::spawn({
            let budget = budget.clone();
            move || {
                let permit = budget.acquire(3);
                tx.send((3, permit)).unwrap();
                let permit = budget.acquire(3);
                tx.send((4, permit)).unwrap();
            }
        });
        assert!(rx.recv_timeout(Duration::from_millis(30)).is_err());
        assert_eq!(budget.state.lock().unwrap().bytes, 6);
        drop(first);
        let (number, third) = rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(number, 3);
        assert!(rx.recv_timeout(Duration::from_millis(30)).is_err());
        drop(second);
        let (number, fourth) = rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(number, 4);
        drop((third, fourth));
        producer.join().unwrap();
        assert_eq!(budget.state.lock().unwrap().bytes, 0);
        assert_eq!(budget.state.lock().unwrap().frames, 0);
    }

    #[test]
    fn input_consumption_stops_at_frame_limit() {
        let bytes = b"123456789\nnext\n";
        let mut reader = io::Cursor::new(bytes);
        assert!(frame(&mut reader, 4).is_err());
        assert_eq!(reader.position(), 5);
        let mut reader = io::Cursor::new(b"{}\n[]\n");
        assert_eq!(frame(&mut reader, 3).unwrap().unwrap(), b"{}\n");
        assert_eq!(frame(&mut reader, 3).unwrap().unwrap(), b"[]\n");
        assert!(frame(&mut reader, 3).unwrap().is_none());
    }
}
