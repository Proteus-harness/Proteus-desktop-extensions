//! Context menu actions: creating, renaming and deleting entries, Git and
//! folder expansion. Failures stay above the tree instead of an error view.

use proteus_extension::{Host, Result, Value, ValueExt, json};

use crate::{Files, paths, tree};

impl Files {
    pub fn operation(&mut self, host: &mut Host, action: &str, value: &Value) {
        let path = value
            .as_str()
            .or_else(|| value.opt_str("value"))
            .unwrap_or("");
        let typed = value.str("text");
        let result = match action {
            "copy" => {
                host.clipboard(path);
                Ok(())
            }
            "document-mode" => {
                if let (Some(path), Some(mode)) = (value[0].as_str(), value[1].as_str()) {
                    self.modes.insert(path.to_owned(), mode.to_owned());
                }
                Ok(())
            }
            "diff" => {
                self.show(host, path, "diff", true);
                Ok(())
            }
            "new-file" | "new-folder" => self.create(host, path, typed, action == "new-file"),
            "rename" => self.rename(host, path, typed),
            "delete" => self.delete(host, path),
            "restore" => self.write(host, "/workspace/discard", json!({"paths": [path]})),
            "ignore" => {
                let (path, target) = (value[0].as_str().unwrap_or(""), value[1].as_str());
                self.write(
                    host,
                    "/workspace/ignore",
                    json!({"paths": [path], "target": target.unwrap_or("gitignore")}),
                )
            }
            "expand-all" => {
                let mut load = |folder: &str| {
                    host.api("/workspace/list", &[("path", folder)])
                        .map(|listing| tree::Listing::parse(&listing))
                };
                self.project.expand_all(path, &mut load);
                Ok(())
            }
            "collapse-all" => {
                self.project.collapse_all(path);
                Ok(())
            }
            _ => Ok(()),
        };
        if let Err(error) = result {
            self.error = Some(error.to_string());
        }
    }

    /// Opens a document of `path` in `mode`, permanently when `focus`.
    pub fn show(&mut self, host: &Host, path: &str, mode: &str, focus: bool) {
        self.selected = Some(path.to_owned());
        self.modes.insert(path.to_owned(), mode.to_owned());
        self.documents.remove(&(path.to_owned(), mode.to_owned()));
        host.open_document(path, !focus, focus);
    }

    /// A POST that changes the workspace; the tree and documents reload after it.
    fn write(&mut self, host: &Host, endpoint: &str, body: Value) -> Result {
        let mut payload = json!({"session_dir": host.session});
        if let (Some(payload), Value::Object(fields)) = (payload.as_object_mut(), body) {
            payload.extend(fields);
        }
        let result = host.post(endpoint, payload);
        self.changed();
        self.documents.clear();
        result?;
        self.error = None;
        Ok(())
    }

    fn create(&mut self, host: &Host, folder: &str, typed: &str, file: bool) -> Result {
        let Some(path) = paths::join(folder, typed) else {
            self.error = Some("Введите имя внутри папки.".to_owned());
            return Ok(());
        };
        let kind = if file { "file" } else { "directory" };
        self.write(
            host,
            "/workspace/create",
            json!({"path": path, "kind": kind}),
        )?;
        self.project.reveal(&path);
        self.selected = Some(path.clone());
        if file {
            self.show(host, &path, "file", true);
        }
        Ok(())
    }

    fn rename(&mut self, host: &mut Host, from: &str, typed: &str) -> Result {
        let Some(to) = paths::join(paths::parent(from), typed) else {
            self.error = Some("Введите новое имя.".to_owned());
            return Ok(());
        };
        if to == from {
            return Ok(());
        }
        if !host.has_feature("documents.retarget.v1") {
            proteus_extension::bail!("Клиент не поддерживает переименование открытых документов");
        }
        self.write(host, "/workspace/rename", json!({"from": from, "to": to}))?;
        self.project.moved(from, &to);
        self.project.reveal(&to);
        if let Some(selected) = &self.selected
            && let Some(moved) = paths::moved(selected, from, &to)
        {
            self.selected = Some(moved);
        }
        self.retarget_documents(host, from, &to)?;
        Ok(())
    }

    fn retarget_documents(&mut self, host: &mut Host, from: &str, to: &str) -> Result {
        for (from, to) in self.moved_documents(from, to) {
            host.retarget_document(&from, &to)?;
        }
        Ok(())
    }

    fn moved_documents(
        &mut self,
        from: &str,
        to: &str,
    ) -> std::collections::BTreeMap<String, String> {
        let mut changes = std::collections::BTreeMap::new();
        for path in self.document_views.values_mut() {
            if let Some(moved) = paths::moved(path, from, to) {
                changes.insert(path.clone(), moved.clone());
                *path = moved;
            }
        }
        let modes = std::mem::take(&mut self.modes);
        let mut moved_modes = Vec::new();
        for (path, mode) in modes {
            if let Some(moved) = paths::moved(&path, from, to) {
                moved_modes.push((moved, mode));
            } else {
                self.modes.insert(path, mode);
            }
        }
        for (path, mode) in moved_modes {
            self.modes.insert(path, mode);
        }
        changes
    }

    fn delete(&mut self, host: &Host, path: &str) -> Result {
        if path.is_empty() {
            return Ok(());
        }
        self.write(host, "/workspace/delete", json!({"paths": [path]}))?;
        self.project.removed(path);
        if self
            .selected
            .as_deref()
            .is_some_and(|selected| paths::within(selected, path))
        {
            self.selected = None;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn renaming_a_folder_retargets_owned_files_and_keeps_modes() {
        let mut files = Files::default();
        for (view, path) in [
            ("one", "src/a.rs"),
            ("two", "src/a.rs"),
            ("nested", "src/deep/b.rs"),
            ("other", "srcs/a.rs"),
        ] {
            files.track_document_view(view, Some(path));
        }
        files.modes.insert("src/a.rs".into(), "diff".into());
        files.modes.insert("src/deep/b.rs".into(), "file".into());
        let changed = files.moved_documents("src", "lib");
        assert_eq!(
            changed.len(),
            2,
            "shared views produce one exact retarget pair"
        );
        assert_eq!(changed["src/a.rs"], "lib/a.rs");
        assert_eq!(changed["src/deep/b.rs"], "lib/deep/b.rs");
        assert_eq!(files.document_views["other"], "srcs/a.rs");
        assert_eq!(files.document_views["one"], "lib/a.rs");
        assert_eq!(files.modes["lib/a.rs"], "diff");
        assert_eq!(files.modes["lib/deep/b.rs"], "file");
        assert!(!files.modes.contains_key("src/a.rs"));
    }
}
