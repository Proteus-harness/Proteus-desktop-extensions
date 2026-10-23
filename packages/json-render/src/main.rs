//! The package interprets completed fenced sources; the host only mounts
//! its public native content surface and keeps the original source intact.

mod catalog;

use anyhow::anyhow;
use proteus_extension::{Extension, Host, Result, Surface, run};

struct Visualizations;

impl Extension for Visualizations {
    fn render(&mut self, host: &mut Host, view: &str, surface: Surface, _: Option<&str>) -> Result {
        if surface != Surface::Content {
            return Err(anyhow!("visualizations require a content surface"));
        }
        let content = host
            .content(view)
            .ok_or_else(|| anyhow!("content source is missing"))?;
        let root = catalog::render(&content.language, &content.source)?;
        host.show(view, root);
        Ok(())
    }
}

fn main() {
    run(Visualizations);
}
