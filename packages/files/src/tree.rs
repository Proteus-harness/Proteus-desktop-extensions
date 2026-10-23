// Адаптировано из Zed: crates/project_panel/src/project_panel.rs
//! Lazily listed project tree with Zed's auto-folded directories: a folder
//! whose only child is a folder shows as one row `a/b/c`.

use std::collections::{HashMap, HashSet, VecDeque};

use proteus_extension::{Result, Value, ValueExt};

use crate::paths;

/// Entries Zed hides by default (`file_scan_exclusions`).
const EXCLUDED: &[&str] = &[
    ".git",
    ".svn",
    ".hg",
    ".jj",
    ".sl",
    ".repo",
    "CVS",
    ".DS_Store",
    "Thumbs.db",
    ".classpath",
    ".settings",
];
/// Longest folded chain and the deepest `Развернуть всё`.
const CHAIN: usize = 32;
/// Listings fetched only to fold collapsed folders, per render.
const LOOKAHEAD: usize = 64;
/// Folders that `Развернуть всё` lists at most.
const EXPAND_ALL: usize = 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Directory,
    File,
    Symlink,
    Other,
}

#[derive(Clone, Debug)]
pub struct Entry {
    pub name: String,
    pub path: String,
    pub kind: Kind,
}

#[derive(Clone, Debug, Default)]
pub struct Listing {
    pub entries: Vec<Entry>,
    pub truncated: bool,
}

impl Listing {
    pub fn parse(value: &Value) -> Self {
        let entries = value
            .list("entries")
            .iter()
            .filter(|entry| !EXCLUDED.contains(&entry.str("name")))
            .map(|entry| Entry {
                name: entry.str("name").to_owned(),
                path: entry.str("path").trim_start_matches("./").to_owned(),
                kind: match entry.str("kind") {
                    "directory" => Kind::Directory,
                    "file" => Kind::File,
                    "symlink" => Kind::Symlink,
                    _ => Kind::Other,
                },
            })
            .collect();
        Self {
            entries,
            truncated: value.bool("truncated"),
        }
    }

    /// The only child when it is a folder.
    fn single_folder(&self) -> Option<&Entry> {
        match self.entries.as_slice() {
            [entry] if entry.kind == Kind::Directory && !self.truncated => Some(entry),
            _ => None,
        }
    }
}

/// A visible row: `path` is the deepest folder of a folded chain.
#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub path: String,
    pub label: String,
    pub depth: usize,
    pub kind: Kind,
    /// `None` for files.
    pub expanded: Option<bool>,
}

pub type Loader<'a> = dyn FnMut(&str) -> Result<Listing> + 'a;

pub struct Project {
    /// Listed folders until the next refresh; `None` — the folder is gone.
    listings: HashMap<String, Option<Listing>>,
    pub expanded: HashSet<String>,
    /// First folder of each folded row by its deepest folder.
    heads: HashMap<String, String>,
    pub truncated: bool,
}

impl Default for Project {
    fn default() -> Self {
        Self {
            listings: HashMap::new(),
            expanded: HashSet::from([String::new()]),
            heads: HashMap::new(),
            truncated: false,
        }
    }
}

impl Project {
    /// Forgets listings; the next render lists the visible folders again.
    /// A partial refresh keeps listings of collapsed folders, which only
    /// decide how rows fold.
    pub fn invalidate(&mut self, full: bool) {
        if full {
            self.listings.clear();
        } else {
            let expanded = &self.expanded;
            self.listings.retain(|path, _| !expanded.contains(path));
        }
    }

    fn listing(&mut self, path: &str, load: &mut Loader) -> Option<&Listing> {
        if !self.listings.contains_key(path) {
            self.listings.insert(path.to_owned(), load(path).ok());
        }
        self.listings[path].as_ref()
    }

    fn cached(&self, path: &str) -> Option<&Listing> {
        self.listings.get(path).and_then(Option::as_ref)
    }

    /// Kind of a listed entry; the session folder is a directory.
    pub fn kind(&self, path: &str) -> Option<Kind> {
        if path.is_empty() {
            return Some(Kind::Directory);
        }
        self.cached(paths::parent(path))?
            .entries
            .iter()
            .find(|entry| entry.path == path)
            .map(|entry| entry.kind)
    }

    /// Visible rows below the root row `root_label`; fails only when the
    /// session folder itself cannot be listed.
    pub fn lines(&mut self, root_label: &str, load: &mut Loader) -> Result<Vec<Line>> {
        if !self.listings.contains_key("") {
            let root = load("")?;
            self.listings.insert(String::new(), Some(root));
        }
        let open = self.expanded.contains("");
        let mut lines = vec![Line {
            path: String::new(),
            label: root_label.to_owned(),
            depth: 0,
            kind: Kind::Directory,
            expanded: Some(open),
        }];
        self.truncated = false;
        self.heads.clear();
        if open {
            let mut budget = LOOKAHEAD;
            self.visit("", 1, load, &mut budget, &mut lines);
        }
        self.prune();
        Ok(lines)
    }

    fn visit(
        &mut self,
        folder: &str,
        depth: usize,
        load: &mut Loader,
        budget: &mut usize,
        lines: &mut Vec<Line>,
    ) {
        let Some(listing) = self.listing(folder, load).cloned() else {
            self.expanded.remove(folder);
            return;
        };
        self.truncated |= listing.truncated;
        for entry in listing.entries {
            if entry.kind != Kind::Directory {
                lines.push(Line {
                    path: entry.path,
                    label: entry.name,
                    depth,
                    kind: entry.kind,
                    expanded: None,
                });
                continue;
            }
            let (path, label) = self.fold(&entry, load, budget);
            let open = self.expanded.contains(&path);
            lines.push(Line {
                path: path.clone(),
                label,
                depth,
                kind: Kind::Directory,
                expanded: Some(open),
            });
            if open {
                self.visit(&path, depth + 1, load, budget, lines);
            }
        }
    }

    /// The deepest folder of the chain starting at `entry` and its `a/b/c` label.
    fn fold(&mut self, entry: &Entry, load: &mut Loader, budget: &mut usize) -> (String, String) {
        let mut path = entry.path.clone();
        let mut label = entry.name.clone();
        for _ in 0..CHAIN {
            if !self.listings.contains_key(&path) {
                if !self.expanded.contains(&path) && *budget == 0 {
                    break;
                }
                *budget = budget.saturating_sub(1);
            }
            let Some(child) = self.listing(&path, load).and_then(Listing::single_folder) else {
                break;
            };
            label = format!("{label}/{}", child.name);
            path = child.path.clone();
        }
        if path != entry.path {
            self.heads.insert(path.clone(), entry.path.clone());
        }
        (path, label)
    }

    /// Expands a row: every folder folded into it stays open, so the row
    /// keeps its folded label without listing ahead.
    pub fn expand(&mut self, path: &str, load: &mut Loader) {
        let mut path = self
            .heads
            .get(path)
            .cloned()
            .unwrap_or_else(|| path.to_owned());
        // The session folder row never folds, like Zed's worktree root.
        if path.is_empty() {
            self.expanded.insert(path);
            return;
        }
        for _ in 0..CHAIN {
            self.expanded.insert(path.clone());
            // A folder that failed to list before gets another try.
            if self.listings.get(&path).is_some_and(Option::is_none) {
                self.listings.remove(&path);
            }
            match self.listing(&path, load).and_then(Listing::single_folder) {
                Some(child) => path = child.path.clone(),
                None => break,
            }
        }
    }

    /// Collapses a row with the folders folded into it.
    pub fn collapse(&mut self, path: &str) {
        let head = self.heads.get(path).map_or(path, String::as_str);
        let chain: Vec<String> = self
            .expanded
            .iter()
            .filter(|open| paths::within(path, open) && paths::within(open, head))
            .cloned()
            .collect();
        for open in chain {
            self.expanded.remove(&open);
        }
    }

    pub fn is_expanded(&self, path: &str) -> bool {
        self.expanded.contains(path)
    }

    /// Opens every folder above `path`.
    pub fn reveal(&mut self, path: &str) {
        self.expanded
            .extend(paths::parents(path).into_iter().map(str::to_owned));
    }

    /// Zed's `Развернуть всё`: every folder below `path`, within limits.
    pub fn expand_all(&mut self, path: &str, load: &mut Loader) {
        let mut queue = VecDeque::from([(path.to_owned(), 0)]);
        let mut listed = 0;
        while let Some((folder, depth)) = queue.pop_front() {
            if listed == EXPAND_ALL {
                break;
            }
            listed += 1;
            self.expanded.insert(folder.clone());
            let Some(listing) = self.listing(&folder, load) else {
                continue;
            };
            if depth < CHAIN {
                queue.extend(
                    listing
                        .entries
                        .iter()
                        .filter(|entry| entry.kind == Kind::Directory)
                        .map(|entry| (entry.path.clone(), depth + 1)),
                );
            }
        }
    }

    /// Zed's `Свернуть всё`; the session folder stays open.
    pub fn collapse_all(&mut self, path: &str) {
        self.expanded
            .retain(|open| open.is_empty() || !paths::within(open, path));
    }

    /// Keeps the open state of folders renamed from `from` to `to`.
    pub fn moved(&mut self, from: &str, to: &str) {
        self.expanded = self
            .expanded
            .drain()
            .map(|open| paths::moved(&open, from, to).unwrap_or(open))
            .collect();
    }

    pub fn removed(&mut self, path: &str) {
        self.expanded
            .retain(|open| open.is_empty() || !paths::within(open, path));
    }

    /// Drops open folders that vanished from a complete listing of their parent.
    fn prune(&mut self) {
        let listings = &self.listings;
        self.expanded.retain(|open| {
            open.is_empty()
                || match listings.get(paths::parent(open)) {
                    Some(None) => false,
                    Some(Some(listing)) if !listing.truncated => listing
                        .entries
                        .iter()
                        .any(|entry| entry.path == *open && entry.kind == Kind::Directory),
                    _ => true,
                }
        });
    }

    /// Whether `path` is still listed (or its parent is unknown).
    pub fn exists(&self, path: &str) -> bool {
        path.is_empty()
            || match self.listings.get(paths::parent(path)) {
                Some(None) => false,
                Some(Some(listing)) if !listing.truncated => {
                    listing.entries.iter().any(|entry| entry.path == path)
                }
                _ => true,
            }
    }
}

#[cfg(test)]
#[path = "tree_tests.rs"]
mod tests;
