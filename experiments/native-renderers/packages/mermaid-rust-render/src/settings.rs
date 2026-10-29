//! Package-owned preferences; the public page/select/input contract needs no SDK changes.
use std::borrow::Cow;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};

const MAX_FILE_BYTES: usize = 4096;
pub(super) const MAX_CUSTOM_RADIUS: f64 = 64.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Mode {
    #[default]
    Inherit,
    Custom,
}

impl Mode {
    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "inherit" => Ok(Self::Inherit),
            "custom" => Ok(Self::Custom),
            _ => bail!("Неизвестный режим скругления"),
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Self::Inherit => "inherit",
            Self::Custom => "custom",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Settings {
    pub mode: Mode,
    pub custom_radius: f64,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            mode: Mode::Inherit,
            custom_radius: 8.0,
        }
    }
}

impl Settings {
    pub fn value(self) -> Value {
        json!({"mode":self.mode.key(), "custom_radius":self.custom_radius})
    }

    fn parse(value: Value) -> Result<Self> {
        let mode = Mode::parse(
            value["mode"]
                .as_str()
                .context("Режим скругления отсутствует")?,
        )?;
        let custom_radius = value["custom_radius"]
            .as_f64()
            .context("Радиус должен быть числом")?;
        validate_radius(custom_radius)?;
        Ok(Self {
            mode,
            custom_radius,
        })
    }

    /// Borrow host theme in inherit mode; override only our own render request in custom mode.
    pub fn theme<'a>(self, host: &'a Value) -> Result<Cow<'a, Value>> {
        if self.mode == Mode::Inherit {
            return Ok(Cow::Borrowed(host));
        }
        validate_radius(self.custom_radius)?;
        ensure!(host.is_object(), "content.context.theme is required");
        let mut theme = host.clone();
        theme["radius_lg"] = json!(self.custom_radius);
        Ok(Cow::Owned(theme))
    }
}

pub(super) fn radius_input(value: &str) -> Result<f64> {
    let radius: f64 = value
        .trim()
        .parse()
        .context("Введите радиус от 0 до 64 px")?;
    validate_radius(radius)?;
    Ok(radius)
}

fn validate_radius(radius: f64) -> Result<()> {
    ensure!(
        radius.is_finite() && (0.0..=MAX_CUSTOM_RADIUS).contains(&radius),
        "Радиус должен быть от 0 до 64 px"
    );
    Ok(())
}

pub(super) fn path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
    Some(base.join("dev.proteus.native/extension-preferences/mermaid-rust-render.json"))
}

pub(super) fn load(path: Option<&Path>) -> Result<Settings> {
    let Some(path) = path else {
        return Ok(Settings::default());
    };
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Settings::default());
        }
        Err(error) => return Err(error).context("Не удалось прочитать настройки Mermaid"),
    };
    let mut bytes = Vec::new();
    file.take(MAX_FILE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() <= MAX_FILE_BYTES,
        "Файл настроек Mermaid слишком велик"
    );
    Settings::parse(serde_json::from_slice(&bytes).context("Некорректный JSON настроек Mermaid")?)
}

pub(super) fn stamp(path: Option<&Path>) -> Result<Option<(u64, std::time::SystemTime)>> {
    let Some(path) = path else {
        return Ok(None);
    };
    match fs::metadata(path) {
        Ok(metadata) => Ok(Some((metadata.len(), metadata.modified()?))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).context("Не удалось проверить настройки Mermaid"),
    }
}

pub(super) fn save(path: &Path, settings: Settings) -> Result<()> {
    validate_radius(settings.custom_radius)?;
    let parent = path.parent().context("Нет каталога настроек Mermaid")?;
    fs::create_dir_all(parent)?;
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let temporary = parent.join(format!(
        ".mermaid-rust-render-{}-{}.tmp",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let result = (|| -> Result<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(&serde_json::to_vec(&settings.value())?)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        fs::rename(&temporary, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result.context("Не удалось сохранить настройки Mermaid")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inherit_tracks_app_while_custom_is_independent_and_defaults_to_eight() {
        let host = json!({"radius_lg":16.0,"foreground":"#abcdef"});
        let defaults = Settings::default();
        assert_eq!(defaults.mode, Mode::Inherit);
        assert_eq!(defaults.custom_radius, 8.0);
        assert!(matches!(defaults.theme(&host).unwrap(), Cow::Borrowed(_)));
        assert_eq!(defaults.theme(&host).unwrap()["radius_lg"], 16.0);
        let custom = Settings {
            mode: Mode::Custom,
            ..defaults
        };
        assert_eq!(custom.theme(&host).unwrap()["radius_lg"], 8.0);
        assert_eq!(
            custom.theme(&json!({"radius_lg":32.0})).unwrap()["radius_lg"],
            8.0
        );
        assert_eq!(host["radius_lg"], 16.0);
        assert_ne!(defaults.value(), custom.value());
    }

    #[test]
    fn radius_validates_pending_text_and_persisted_values() {
        for invalid in ["", "-1", "65", "NaN", "inf", "eight"] {
            assert!(radius_input(invalid).is_err());
        }
        for valid in ["0", "8", "12.5", " 64 "] {
            assert!(radius_input(valid).is_ok());
        }
        assert!(Settings::parse(json!({"mode":"custom","custom_radius":65})).is_err());
        assert!(Settings::parse(json!({"mode":"invalid","custom_radius":8})).is_err());
    }

    #[test]
    fn actual_custom_diagram_is_independent_of_app_radius_while_inherit_tracks_it() {
        let mut host = json!({"foreground":"#abcdef", "background":"#111213", "muted":"#8a8b8c",
            "border":"#414243", "accent":"#2789fa", "danger":"#ee5566",
            "surface":"#222324", "surface_alt":"#303132", "font_family":"DejaVu Sans", "radius_lg":16.0});
        let source = "flowchart LR\n A[Box]\n B{Choice}";
        let custom = Settings {
            mode: Mode::Custom,
            custom_radius: 4.0,
        };
        let first = crate::render::render(source, &custom.theme(&host).unwrap()).unwrap();
        let inherited =
            crate::render::render(source, &Settings::default().theme(&host).unwrap()).unwrap();
        host["radius_lg"] = json!(32.0);
        assert_eq!(
            first,
            crate::render::render(source, &custom.theme(&host).unwrap()).unwrap()
        );
        assert_ne!(
            inherited,
            crate::render::render(source, &Settings::default().theme(&host).unwrap()).unwrap()
        );
    }

    #[test]
    fn preferences_roundtrip_is_atomic_bounded_and_independent_of_the_package() {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "proteus-mermaid-prefs-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        let path = root.join("extension-preferences/mermaid-rust-render.json");
        assert_eq!(load(Some(&path)).unwrap(), Settings::default());
        let custom = Settings {
            mode: Mode::Custom,
            custom_radius: 4.0,
        };
        save(&path, custom).unwrap();
        assert_eq!(load(Some(&path)).unwrap(), custom);
        assert_eq!(fs::read_dir(path.parent().unwrap()).unwrap().count(), 1);
        fs::write(&path, "x".repeat(MAX_FILE_BYTES + 1)).unwrap();
        assert!(load(Some(&path)).is_err());
        save(&path, Settings::default()).unwrap();
        assert_eq!(load(Some(&path)).unwrap(), Settings::default());
        fs::remove_dir_all(root).unwrap();
    }
}
