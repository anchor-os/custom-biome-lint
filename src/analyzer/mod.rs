pub mod file_matcher;
pub mod runner;

use std::fs;
use std::path::{Path, PathBuf};

use rayon::prelude::*;

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
///
/// Subdirectories are walked concurrently on rayon's global thread pool: each
/// directory's `read_dir` and match check is small, but a tree with hundreds
/// of directories turns into hundreds of blocking syscalls if done one at a
/// time, so this fans them out and merges the per-directory results.
pub fn discover_files(pattern: &GlobSet, root: &Path) -> Discovery {
    let start = root.join(pattern.root_dir());

    if start.is_file() {
        return Discovery {
            files: vec![start],
            dirs_scanned: 0,
            dirs_skipped: Vec::new(),
            files_considered: 1,
        };
    }

    let mut discovery = walk(&start, root, pattern);
    discovery.files.sort();
    discovery
}

fn walk(dir: &Path, root: &Path, pattern: &GlobSet) -> Discovery {
    let mut discovery = Discovery::default();
    let Ok(entries) = fs::read_dir(dir) else {
        return discovery;
    };
    discovery.dirs_scanned += 1;

    let mut subdirs = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();

        if path.is_dir() {
            if SKIPPED_DIRS.contains(&name.as_ref()) || name.starts_with('.') {
                discovery.dirs_skipped.push(path);
                continue;
            }
            subdirs.push(path);
        } else {
            discovery.files_considered += 1;
            if pattern.is_match(&file_matcher::match_key(&path, root, pattern)) {
                discovery.files.push(path);
            }
        }
    }

    let merged = subdirs
        .into_par_iter()
        .map(|path| walk(&path, root, pattern))
        .reduce(Discovery::default, |mut a, b| {
            a.dirs_scanned += b.dirs_scanned;
            a.files_considered += b.files_considered;
            a.files.extend(b.files);
            a.dirs_skipped.extend(b.dirs_skipped);
            a
        });

    discovery.dirs_scanned += merged.dirs_scanned;
    discovery.files_considered += merged.files_considered;
    discovery.files.extend(merged.files);
    discovery.dirs_skipped.extend(merged.dirs_skipped);

    discovery
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
