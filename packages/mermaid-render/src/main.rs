//! Mermaid remains a standalone process package. The host receives generic SVG.
mod cache;
mod worker;

use std::collections::HashSet;
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock, Weak};

use anyhow::{anyhow, ensure};
use proteus_extension::{Extension, Host, Result, Surface, run};
use serde_json::json;

struct Mermaid {
    shared: Arc<Mutex<Pool>>,
}

impl Default for Mermaid {
    fn default() -> Self {
        // SDK instances belong to individual sessions; the renderer belongs to
        // the package process. A Weak registry lets the last instance drop it.
        static SHARED: OnceLock<Mutex<Weak<Mutex<Pool>>>> = OnceLock::new();
        let mut registry = SHARED
            .get_or_init(Mutex::default)
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let shared = registry.upgrade().unwrap_or_else(|| {
            let shared = Arc::new(Mutex::new(Pool::default()));
            *registry = Arc::downgrade(&shared);
            shared
        });
        Self { shared }
    }
}

#[derive(Default)]
struct Pool {
    worker: Option<worker::Worker>,
    views: HashSet<String>,
    cache: cache::Cache,
}

impl Extension for Mermaid {
    fn render(&mut self, host: &mut Host, view: &str, surface: Surface, _: Option<&str>) -> Result {
        self.shared
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .render(host, view, surface)
    }

    fn closed(&mut self, _host: &mut Host, view: &str) {
        self.shared
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .closed(view);
    }
}

impl Pool {
    fn render(&mut self, host: &mut Host, view: &str, surface: Surface) -> Result {
        ensure!(
            surface == Surface::Content,
            "Mermaid requires a content surface"
        );
        let content = host
            .content(view)
            .ok_or_else(|| anyhow!("content source is missing"))?;
        ensure!(
            content.language == "mermaid",
            "unsupported Mermaid language"
        );
        ensure!(
            content.source.len() <= 50_000,
            "Mermaid source exceeds 50,000 bytes"
        );
        let request = json!({"source": content.source, "theme": content.context.get("theme")});
        let key = request.to_string();
        ensure!(key.len() <= 512 * 1024, "Mermaid request is too large");
        self.views.insert(view.to_owned());
        if let Some(root) = self.cache.get(&key, view) {
            host.show(view, root);
            return Ok(());
        }
        if self.worker.is_none() {
            // The public extension launcher sets cwd to the package directory.
            self.worker = Some(worker::Worker::start(Path::new("worker.mjs"))?);
        }
        let root = match self.worker.as_mut().unwrap().render(&key) {
            Ok(root) => root,
            Err(error) => {
                self.worker.take();
                return Err(error);
            }
        };
        self.cache.insert(key, view, root.clone());
        host.show(view, root);
        Ok(())
    }

    fn closed(&mut self, view: &str) {
        self.views.remove(view);
        self.cache.closed(view);
        if self.views.is_empty() {
            self.worker.take(); // Drop kills and waits; no orphan renderer remains.
            self.cache = cache::Cache::default();
        }
    }
}

fn main() {
    run(Mermaid::default());
}
