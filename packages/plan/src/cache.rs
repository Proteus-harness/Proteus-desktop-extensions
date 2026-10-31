//! One authoritative compact lookup, owned only by the current session.
//! No polling, raw tool retention, transcript copy, or history traversal.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::io::Write;

use proteus_extension::{ApiError, Host, Result, Value, ValueExt, anyhow};

use crate::model::Snapshot;

#[derive(Debug, PartialEq, Eq)]
struct Key {
    session: Option<String>,
    snapshot_epoch: u64,
    fingerprint: u64,
}

struct HashWriter<'a>(&'a mut DefaultHasher);

impl Write for HashWriter<'_> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.write(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn key(host: &Host) -> Key {
    let mut fingerprint = DefaultHasher::new();
    for (index, entry) in host.entries.iter().enumerate() {
        if entry.str("role") == "user" {
            index.hash(&mut fingerprint);
        }
    }
    let latest = host
        .entries
        .iter()
        .rev()
        .map(|entry| entry.at("tool"))
        .find(|tool| tool.str("name") == "update_plan");
    latest.is_some().hash(&mut fingerprint);
    if let Some(tool) = latest {
        for field in ["call_id", "status", "args", "effective_args"] {
            // Hash encoded bytes without allocating a second JSON string or
            // retaining arguments; values are already SDK-owned immutable input.
            serde_json::to_writer(HashWriter(&mut fingerprint), tool.at(field))
                .expect("writing JSON to a hasher cannot fail");
        }
    }
    let execution = host.state.as_ref().map(|state| state.at("execution"));
    for run in ["active", "last"] {
        for field in ["run_id", "status"] {
            let value = execution
                .map(|execution| execution.at(run).at(field))
                .unwrap_or(&Value::Null);
            serde_json::to_writer(HashWriter(&mut fingerprint), value)
                .expect("writing JSON to a hasher cannot fail");
        }
    }
    Key {
        session: host.session.clone(),
        snapshot_epoch: host.session_snapshot_epoch(),
        fingerprint: fingerprint.finish(),
    }
}

enum Outcome {
    Ready(Option<Snapshot>),
    NotReady(String),
    Failed(String),
}

struct Attempt {
    key: Key,
    event_epoch: u64,
    outcome: Outcome,
}

#[derive(Default)]
pub struct Cache {
    attempt: Option<Attempt>,
}

impl Cache {
    pub fn clear(&mut self) {
        self.attempt = None;
    }

    /// Explicit user retry permits one new compact read even on an idle chat.
    pub fn retry(&mut self) {
        self.clear();
    }

    pub fn always(&mut self, host: &Host) -> Result<Option<&Snapshot>> {
        self.lookup(key(host), host.session_event_epoch(), || {
            host.latest_tool_state("update_plan")
        })
    }

    fn lookup(
        &mut self,
        key: Key,
        event_epoch: u64,
        load: impl FnOnce() -> Result<Option<Value>>,
    ) -> Result<Option<&Snapshot>> {
        let reuse = self.attempt.as_ref().is_some_and(|attempt| {
            attempt.key == key
                && (matches!(attempt.outcome, Outcome::Ready(_))
                    || attempt.event_epoch == event_epoch)
        });
        if !reuse {
            // Discard the previous session/projection before making a new read.
            self.clear();
            let outcome = match load() {
                Ok(tool) => Outcome::Ready(tool.and_then(|tool| {
                    let plan = Snapshot::from_tool(&tool);
                    (!plan.steps.is_empty()).then_some(plan)
                })),
                Err(error)
                    if error
                        .downcast_ref::<ApiError>()
                        .is_some_and(|error| error.status == 409) =>
                {
                    Outcome::NotReady(error.to_string())
                }
                Err(error) => {
                    Outcome::Failed(format!("Не удалось прочитать последний план: {error:#}"))
                }
            };
            self.attempt = Some(Attempt {
                key,
                event_epoch,
                outcome,
            });
        }
        match &self
            .attempt
            .as_ref()
            .expect("lookup stored an attempt")
            .outcome
        {
            Outcome::Ready(plan) => Ok(plan.as_ref()),
            Outcome::NotReady(message) => Err(ApiError {
                status: 409,
                message: message.clone(),
            }
            .into()),
            Outcome::Failed(message) => Err(anyhow!(message.clone())),
        }
    }
}

#[cfg(test)]
mod tests;
