//! Read-only presentation values supplied by the owning Desktop window.

use std::io;

use crate::{Map, Result, Value};

pub(crate) const MAX_BYTES: usize = 64 * 1024;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Preferences {
    values: Map<String, Value>,
}

impl Preferences {
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.values.get(key)
    }

    pub fn from_values(values: Map<String, Value>) -> Result<Self> {
        check_bytes(&values)?;
        Ok(Self { values })
    }

    pub(crate) fn values(&self) -> &Map<String, Value> {
        &self.values
    }
}

pub(crate) fn check_bytes(values: &Map<String, Value>) -> Result {
    serde_json::to_writer(Counter(0), values)?;
    Ok(())
}

pub(crate) fn check_pending(values: &[(&str, Option<&Value>)]) -> Result {
    serde_json::to_writer(Counter(0), values)?;
    Ok(())
}

struct Counter(usize);
impl io::Write for Counter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0 = self
            .0
            .checked_add(bytes.len())
            .filter(|size| *size <= MAX_BYTES)
            .ok_or_else(|| io::Error::other("visual preferences exceed 64 KiB"))?;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json;

    #[test]
    fn snapshots_are_read_only_and_encoded_byte_limit_handles_escaping() {
        let preferences =
            Preferences::from_values(Map::from_iter([("visibility".into(), json!("always"))]))
                .unwrap();
        assert_eq!(preferences.get("visibility"), Some(&json!("always")));
        assert_eq!(Preferences::default().get("visibility"), None);
        assert!(
            Preferences::from_values(Map::from_iter([(
                "text".into(),
                json!("\n".repeat(MAX_BYTES / 2))
            ),]))
            .is_err()
        );
        Preferences::from_values(Map::from_iter([("text".into(), json!("é".repeat(100)))]))
            .unwrap();
    }
}
