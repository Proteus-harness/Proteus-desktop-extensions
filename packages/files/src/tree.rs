// Адаптировано из Zed: crates/project_panel/src/project_panel.rs
//! Lazily listed project tree with Zed's auto-folded directories: a folder
//! whose only child is a folder shows as one row `a/b/c`.

use std::collections::{HashMap, HashSet, VecDeque};
use std::rc::Rc;

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

#[derive(Debug)]
#[cfg_attr(test, derive(Clone))]
struct Entry {
    name: EntryName,
    path: String,
    kind: Kind,
}

#[derive(Debug)]
#[cfg_attr(test, derive(Clone))]
enum EntryName {
    PathSuffix(usize),
    Override(Box<str>),
}

impl Entry {
    fn new(name: &str, path: String, kind: Kind) -> Self {
        // Reuse bytes only when the exact display name is already in the
        // immutable normalized path. An alias keeps its original label.
        let name = match path.strip_suffix(name) {
            Some(prefix) if prefix.is_empty() || prefix.ends_with(['/', '\\']) => {
                EntryName::PathSuffix(prefix.len())
            }
            _ => EntryName::Override(name.into()),
        };
        Self { name, path, kind }
    }

    fn name(&self) -> &str {
        match &self.name {
            EntryName::PathSuffix(start) => &self.path[*start..],
            EntryName::Override(name) => name,
        }
    }
}

#[derive(Debug, Default)]
#[cfg_attr(test, derive(Clone))]
pub struct Listing {
    entries: Vec<Entry>,
    pub truncated: bool,
}

fn take_path(entry: &mut Value) -> String {
    match entry.get_mut("path") {
        // Keep the same path capacity as the former normalized copy.
        Some(Value::String(path)) if path.capacity() == path.len() && !path.starts_with("./") => {
            std::mem::take(path)
        }
        Some(Value::String(path)) => path.trim_start_matches("./").to_owned(),
        _ => String::new(),
    }
}

impl Listing {
    pub fn parse(mut value: Value) -> Self {
        let entries = value
            .get_mut("entries")
            .and_then(Value::as_array_mut)
            .map(Vec::as_mut_slice)
            .unwrap_or_default()
            .iter_mut()
            .filter(|entry| !EXCLUDED.contains(&entry.str("name")))
            .map(|entry| {
                let kind = match entry.str("kind") {
                    "directory" => Kind::Directory,
                    "file" => Kind::File,
                    "symlink" => Kind::Symlink,
                    _ => Kind::Other,
                };
                let path = take_path(entry);
                Entry::new(entry.str("name"), path, kind)
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
    /// Listings until refresh; `None` memoizes a failed attempt, not absence.
    listings: HashMap<String, Option<Rc<Listing>>>,
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
    /// decide how rows fold. Failed attempts expire even for collapsed folders.
    pub fn invalidate(&mut self, full: bool) {
        if full {
            self.listings.clear();
        } else {
            let expanded = &self.expanded;
            self.listings
                .retain(|path, listing| listing.is_some() && !expanded.contains(path));
        }
    }

    fn listing(&mut self, path: &str, load: &mut Loader) -> Option<&Rc<Listing>> {
        if !self.listings.contains_key(path) {
            self.listings
                .insert(path.to_owned(), load(path).ok().map(Rc::new));
        }
        self.listings[path].as_ref()
    }

    fn cached(&self, path: &str) -> Option<&Listing> {
        self.listings.get(path).and_then(Option::as_deref)
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
            self.listings.insert(String::new(), Some(Rc::new(root)));
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
        // Keep just this immutable listing alive while lookahead/recursion
        // inserts other listings into the map; no copy of its entries.
        let Some(listing) = self.listing(folder, load).cloned() else {
            return;
        };
        self.truncated |= listing.truncated;
        for entry in &listing.entries {
            if entry.kind != Kind::Directory {
                lines.push(Line {
                    path: entry.path.clone(),
                    label: entry.name().to_owned(),
                    depth,
                    kind: entry.kind,
                    expanded: None,
                });
                continue;
            }
            let (path, label) = self.fold(entry, load, budget);
            let open = self.expanded.contains(&path);
            let expanded_path = open.then(|| path.clone());
            lines.push(Line {
                path,
                label,
                depth,
                kind: Kind::Directory,
                expanded: Some(open),
            });
            if let Some(expanded_path) = expanded_path {
                self.visit(&expanded_path, depth + 1, load, budget, lines);
            }
        }
    }

    /// The deepest folder of the chain starting at `entry` and its `a/b/c` label.
    fn fold(&mut self, entry: &Entry, load: &mut Loader, budget: &mut usize) -> (String, String) {
        let mut path = entry.path.clone();
        let mut label = entry.name().to_owned();
        for _ in 0..CHAIN {
            if !self.listings.contains_key(&path) {
                if !self.expanded.contains(&path) && *budget == 0 {
                    break;
                }
                *budget = budget.saturating_sub(1);
            }
            let Some(child) = self
                .listing(&path, load)
                .and_then(|listing| listing.single_folder())
            else {
                break;
            };
            label.push('/');
            label.push_str(child.name());
            path.clone_from(&child.path);
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
            match self
                .listing(&path, load)
                .and_then(|listing| listing.single_folder())
            {
                Some(child) => path = child.path.clone(),
                None => break,
            }
        }
    }

    /// Collapses a row with the folders folded into it.
    pub fn collapse(&mut self, path: &str) {
        let head = self.heads.get(path).map_or(path, String::as_str);
        let mut open = path;
        // Only the path and its ancestors can be part of this folded row.
        while paths::within(open, head) {
            self.expanded.remove(open);
            if open == head {
                break;
            }
            open = paths::parent(open);
        }
    }

    pub fn is_expanded(&self, path: &str) -> bool {
        self.expanded.contains(path)
    }

    /// Opens every folder above `path`.
    pub fn reveal(&mut self, path: &str) {
        self.expanded
            .extend(paths::parents(path).map(str::to_owned));
    }

    /// Zed's `Развернуть всё`: every folder below `path`, within limits.
    pub fn expand_all(&mut self, path: &str, load: &mut Loader) {
        // The row's folded ancestors share its expansion/refresh intent.
        // Do not list them here or spend the descendant traversal budget.
        if let Some(head) = self.heads.get(path) {
            self.expanded.extend(
                paths::parents(path)
                    .take_while(|parent| paths::within(parent, head))
                    .map(str::to_owned),
            );
        }
        let mut queue = VecDeque::from([(path.to_owned(), 0)]);
        let mut listed = 0;
        while let Some((folder, depth)) = queue.pop_front() {
            listed += 1;
            self.expanded.insert(folder.clone());
            let Some(listing) = self.listing(&folder, load) else {
                continue;
            };
            if depth < CHAIN {
                // Already queued folders have priority over these children.
                let remaining = EXPAND_ALL - listed - queue.len();
                queue.extend(
                    listing
                        .entries
                        .iter()
                        .filter(|entry| entry.kind == Kind::Directory)
                        .take(remaining)
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
