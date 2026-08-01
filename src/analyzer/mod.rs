pub mod file_matcher;
pub mod runner;

use std::fs;
use std::path::{Path, PathBuf};

pub use file_matcher::GlobSet;
pub use runner::{analyze_file, AnalyzedFile, FileContext};

/// Directories never worth walking into.
const SKIPPED_DIRS: &[&str] = &[
    "node_modules",
    "target",
    "dist",
    "build",
    "coverage",
    "vendor",
];

#[derive(Debug, Default)]
pub struct Discovery {
    pub files: Vec<PathBuf>,
    pub dirs_scanned: usize,
    pub dirs_skipped: Vec<PathBuf>,
    pub files_considered: usize,
}

/// Walks `root`-relative paths under the pattern's literal prefix and keeps the
/// ones the pattern matches. Returns paths relative to `root`, sorted.
pub fn discover_files(pattern: &GlobSet, root: &Path) -> Discovery {
    let mut discovery = Discovery::default();
    let start = root.join(pattern.root_dir());

    if start.is_file() {
        discovery.files_considered += 1;
        discovery.files.push(start.clone());
        return discovery;
    }

    walk(&start, root, pattern, &mut discovery);
    discovery.files.sort();
    discovery
}

fn walk(dir: &Path, root: &Path, pattern: &GlobSet, discovery: &mut Discovery) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    discovery.dirs_scanned += 1;

    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();

        if path.is_dir() {
            if SKIPPED_DIRS.contains(&name.as_ref()) || name.starts_with('.') {
                discovery.dirs_skipped.push(path);
                continue;
            }
            walk(&path, root, pattern, discovery);
        } else {
            discovery.files_considered += 1;
            if pattern.is_match(&file_matcher::match_key(&path, root, pattern)) {
                discovery.files.push(path);
            }
        }
    }
}

/// Accepts a bare directory as shorthand for "every supported file under it",
/// so `custom-biome-lint src` behaves the way the old reselect-lint CLI did
/// instead of silently matching nothing.
pub fn resolve_pattern(input: &str, root: &Path, default_extensions: &str) -> GlobSet {
    if !file_matcher::has_magic(input) && root.join(input).is_dir() {
        let trimmed = input.trim_end_matches('/');
        return GlobSet::new(&format!("{trimmed}/**/*.{{{default_extensions}}}"));
    }
    GlobSet::new(input)
}
