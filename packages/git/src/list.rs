//! Clicks and checkboxes of the changes tree.

use proteus_extension::{Host, Value, ValueExt, json};

use crate::document::Document;
use crate::model::{self, Section, Staging};
use crate::{Git, panel};

impl Git {
    pub fn tree(&mut self, host: &Host, identity: &str, event: &str, entries: &[Value]) {
        if let Some(section) = identity.strip_prefix("section:").and_then(Section::parse) {
            let key = section.key();
            let open = match event {
                "check" => return self.check_section(host, section, entries),
                "expand" => true,
                "collapse" => false,
                "select" | "open" => self.collapsed.contains(key),
                _ => return,
            };
            if open {
                self.collapsed.remove(key);
            } else {
                self.collapsed.insert(key);
            }
            return;
        }
        let path = panel::row_path(identity);
        let Some(entry) = entries.iter().find(|entry| entry.str("path") == path) else {
            return;
        };
        self.selected = Some(identity.to_owned());
        match event {
            "check" => {
                let unstage = match identity.split_once(':') {
                    Some(("staged", _)) => true,
                    Some(("unstaged", _)) => false,
                    _ => model::staging(entry) == Staging::Staged,
                };
                let path_name = if unstage { "unstage" } else { "stage" };
                self.write(
                    host,
                    &format!("/workspace/{path_name}"),
                    json!({"paths": [path]}),
                );
            }
            "select" | "preview" | "open" => {
                let argument = Document::Diff(path.to_owned()).argument();
                host.browse_document(&argument, event);
            }
            _ => {}
        }
    }

    fn check_section(&mut self, host: &Host, section: Section, entries: &[Value]) {
        let sections = model::sections(entries, self.group);
        let Some((_, items)) = sections.iter().find(|(key, _)| *key == section) else {
            return;
        };
        let unstage = model::section_staging(section, items) == Staging::Staged;
        let paths: Vec<&str> = items
            .iter()
            .filter(|entry| {
                let field = if unstage { "staged" } else { "unstaged" };
                entry.at(field).truthy()
            })
            .map(|entry| entry.str("path"))
            .collect();
        if !paths.is_empty() {
            let path = if unstage { "unstage" } else { "stage" };
            self.write(host, &format!("/workspace/{path}"), json!({"paths": paths}));
        }
    }
}
