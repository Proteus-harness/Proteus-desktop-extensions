//! Opt-in math fences, rendered by latex-rust; no browser or JavaScript runtime.
#[path = "../../../../../packages/mermaid-render/src/cache.rs"]
mod cache;
#[cfg(test)]
mod geometry_tests;
mod render;

use std::collections::HashSet;
use std::sync::{Arc, Mutex, OnceLock, Weak};

use anyhow::{anyhow, ensure};
use latex_rust::MathFont;
use proteus_extension::{Extension, Host, Result, Surface, run};
use serde_json::json;

struct Latex {
    shared: Arc<Mutex<Pool>>,
}

impl Default for Latex {
    fn default() -> Self {
        // Instances belong to sessions; a weak registry releases retained state
        // after the last session, and closed() releases it after the last view.
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
    font: Option<MathFont>,
    views: HashSet<String>,
    cache: cache::Cache,
}

impl Extension for Latex {
    fn render(&mut self, host: &mut Host, view: &str, surface: Surface, _: Option<&str>) -> Result {
        ensure!(
            surface == Surface::Content,
            "LaTeX requires a content surface"
        );
        let content = host
            .content(view)
            .ok_or_else(|| anyhow!("content source is missing"))?;
        ensure!(
            matches!(content.language.as_str(), "latex" | "tex"),
            "unsupported LaTeX language"
        );
        render::check_source(&content.source)?;
        let style = render::Style::from_context(&content.context)?;
        let key = json!([
            content.source,
            style.color.css_hex(),
            style.opacity,
            style.font_size_px
        ])
        .to_string();
        let mut shared = self
            .shared
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        shared.views.insert(view.to_owned());
        if let Some(root) = shared.cache.get(&key, view) {
            host.show(view, root);
            return Ok(());
        }
        if shared.font.is_none() {
            shared.font = Some(MathFont::stix_two_math()?);
        }
        let root = render::render(&content.source, shared.font.as_ref().unwrap(), &style)?;
        shared.cache.insert(key, view, root.clone());
        host.show(view, root);
        Ok(())
    }

    fn closed(&mut self, _: &mut Host, view: &str) {
        self.shared
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .closed(view);
    }
}

impl Pool {
    fn closed(&mut self, view: &str) {
        self.views.remove(view);
        self.cache.closed(view);
        if self.views.is_empty() {
            self.font.take();
            self.cache = cache::Cache::default();
        }
    }
}

fn main() {
    run(Latex::default());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn font_is_shared_and_released_after_the_last_view() {
        let mut pool = Pool::default();
        pool.font = Some(MathFont::stix_two_math().unwrap());
        pool.views.extend(["one".into(), "two".into()]);
        pool.closed("one");
        assert!(pool.font.is_some());
        pool.closed("two");
        assert!(pool.font.is_none());
        let first = Latex::default();
        let second = Latex::default();
        assert!(Arc::ptr_eq(&first.shared, &second.shared));
    }
}
