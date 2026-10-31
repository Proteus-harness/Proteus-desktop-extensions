//! Small package-owned preferences, independent of session settings.

use std::path::PathBuf;

use crate::{Result, Value};

pub struct Preferences {
    path: Option<PathBuf>,
    values: serde_json::Map<String, Value>,
}

impl Preferences {
    /// Reads `$XDG_CONFIG_HOME/dev.proteus.native/extension-preferences/<package>.json`.
    /// Missing or invalid files use defaults; writing reports errors.
    pub fn load(package: &str) -> Self {
        let valid = !package.is_empty()
            && package
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"-_".contains(&byte));
        let path = valid
            .then(|| {
                std::env::var_os("XDG_CONFIG_HOME")
                    .map(PathBuf::from)
                    .filter(|path| path.is_absolute())
                    .or_else(|| {
                        std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config"))
                    })
                    .map(|root| {
                        root.join("dev.proteus.native/extension-preferences")
                            .join(format!("{package}.json"))
                    })
            })
            .flatten();
        Self::from_path(path)
    }

    fn from_path(path: Option<PathBuf>) -> Self {
        let values = path
            .as_ref()
            .and_then(|path| std::fs::read(path).ok())
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        Self { path, values }
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        self.values.get(key)
    }

    pub fn set(&mut self, key: &str, value: Value) -> Result {
        let mut values = self.values.clone();
        values.insert(key.into(), value);
        self.save(values)
    }

    pub fn remove(&mut self, key: &str) -> Result {
        let mut values = self.values.clone();
        values.remove(key);
        self.save(values)
    }

    pub fn reset(&mut self) -> Result {
        self.save(Default::default())
    }

    fn save(&mut self, values: serde_json::Map<String, Value>) -> Result {
        let path = self
            .path
            .as_ref()
            .ok_or_else(|| crate::anyhow!("папка настроек недоступна"))?;
        std::fs::create_dir_all(path.parent().expect("package preference parent"))?;
        // Each process gets a separate staging file; readers see only complete JSON.
        let pending = path.with_extension(format!("{}.pending", std::process::id()));
        std::fs::write(&pending, serde_json::to_vec_pretty(&values)?)?;
        if let Err(error) = std::fs::rename(&pending, path) {
            let _ = std::fs::remove_file(&pending);
            return Err(error.into());
        }
        self.values = values;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json;

    #[test]
    fn roundtrip_and_individual_reset_preserve_other_parameters() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../build/test-preferences")
            .join(format!(
                "{}-{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
        let path = root.join("sample.json");
        let mut preferences = Preferences::from_path(Some(path.clone()));
        preferences.set("visibility", json!("always")).unwrap();
        preferences.set("other", json!(42)).unwrap();
        assert_eq!(
            Preferences::from_path(Some(path.clone())).get("visibility"),
            Some(&json!("always"))
        );
        preferences.remove("visibility").unwrap();
        let mut reopened = Preferences::from_path(Some(path.clone()));
        assert!(reopened.get("visibility").is_none());
        assert_eq!(reopened.get("other"), Some(&json!(42)));
        reopened.reset().unwrap();
        assert!(Preferences::from_path(Some(path)).values.is_empty());
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn failed_save_preserves_in_memory_state() {
        let mut preferences = Preferences::from_path(None);
        preferences
            .values
            .insert("visibility".into(), json!("current"));
        assert!(preferences.set("visibility", json!("always")).is_err());
        assert!(preferences.remove("visibility").is_err());
        assert_eq!(preferences.get("visibility"), Some(&json!("current")));
        assert!(Preferences::load("../invalid").path.is_none());
    }
}
