//! SDK процессных расширений Proteus.
//!
//! Расширение — отдельная программа: клиент пишет ей JSON-строки в stdin и
//! читает ответы из stdout (см. PROTOCOL.md). [`run`] ведёт этот цикл,
//! [`Host`] хранит состояние сессии и отправляет сообщения, [`ui`] строит
//! виджеты, [`Analytics`] помогает страницам, которые читают HTTP API сервера.

mod analytics;
mod clock;
mod format;
mod host;
pub mod ui;
mod value;

pub use analytics::Analytics;
pub use anyhow::{Context as _, Error, anyhow, bail};
pub use clock::{LocalTime, local, local_time, utc};
pub use format::{exact, file_icon, tokens};
pub use host::{Content, Host, Server, run};
pub use serde_json::{Map, Value, json};
pub use value::ValueExt;

pub type Result<T = ()> = anyhow::Result<T>;

/// Где показан вид расширения.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Surface {
    Page,
    Panel,
    Document,
    Badge,
    Popup,
    Content,
}

impl Surface {
    fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "page" => Self::Page,
            "panel" => Self::Panel,
            "document" => Self::Document,
            "badge" => Self::Badge,
            "popup" => Self::Popup,
            "content" => Self::Content,
            _ => return None,
        })
    }
}

/// Поведение расширения. Ошибка или паника в `render`/`action` не роняет
/// процесс: SDK показывает её поверх последнего удачного вида.
pub trait Extension {
    /// Как часто перерисовывать открытые виды без сообщений клиента.
    fn poll(&self) -> Option<std::time::Duration> {
        None
    }

    fn render(
        &mut self,
        host: &mut Host,
        view: &str,
        surface: Surface,
        argument: Option<&str>,
    ) -> Result;

    fn action(&mut self, _host: &mut Host, _view: &str, _action: &str, _value: &Value) -> Result {
        Ok(())
    }

    /// Событие клиента (`session_opened`, `document_changed` …) до перерисовки.
    fn event(&mut self, _host: &mut Host, _event: &str) {}

    /// Активный чат переключился на другую сессию: сбросьте выбор старой.
    fn session_changed(&mut self, _host: &mut Host) {}
}
