//! Fuzzy file finder: recursive workspace scan + subsequence scoring.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

const MAX_FILES: usize = 20_000;
const SKIP_DIRS: &[&str] = &[
    ".git",
    "target",
    "node_modules",
    "dist",
    "build",
    "__pycache__",
    ".venv",
    "venv",
    ".idea",
    ".vscode",
    "out",
    "bin",
    "obj",
];

pub struct Finder {
    pub files: Vec<String>, // workspace-relative, '/'-separated
    pub query: String,
    pub matched: Vec<usize>, // indices into `files`, best first
    pub sel: usize,
    pub scroll: usize,
    scanned_at: Option<Instant>,
}

impl Finder {
    pub fn new() -> Self {
        Finder {
            files: Vec::new(),
            query: String::new(),
            matched: Vec::new(),
            sel: 0,
            scroll: 0,
            scanned_at: None,
        }
    }

    pub fn open(&mut self, root: &Path) {
        let stale = self.scanned_at.is_none_or(|t| t.elapsed().as_secs() > 10);
        if stale {
            self.files = scan(root);
            self.scanned_at = Some(Instant::now());
        }
        self.query.clear();
        self.sel = 0;
        self.scroll = 0;
        self.refresh();
    }

    pub fn refresh(&mut self) {
        let mut scored: Vec<(i64, usize)> = self
            .files
            .iter()
            .enumerate()
            .filter_map(|(i, f)| fuzzy_score(&self.query, f).map(|s| (s, i)))
            .collect();
        scored.sort_by(|a, b| {
            b.0.cmp(&a.0)
                .then_with(|| self.files[a.1].len().cmp(&self.files[b.1].len()))
        });
        scored.truncate(500);
        self.matched = scored.into_iter().map(|(_, i)| i).collect();
        self.sel = 0;
        self.scroll = 0;
    }

    pub fn selected_path(&self, root: &Path) -> Option<PathBuf> {
        let idx = *self.matched.get(self.sel)?;
        Some(root.join(&self.files[idx]))
    }
}

fn scan(root: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        if out.len() >= MAX_FILES {
            break;
        }
        let Ok(rd) = fs::read_dir(&dir) else { continue };
        for entry in rd.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if path.is_dir() {
                if name.starts_with('.') || SKIP_DIRS.contains(&name.as_str()) {
                    continue;
                }
                stack.push(path);
            } else {
                let rel = path
                    .strip_prefix(root)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .replace('\\', "/");
                out.push(rel);
                if out.len() >= MAX_FILES {
                    break;
                }
            }
        }
    }
    out.sort();
    out
}

/// Case-insensitive subsequence match with bonuses for consecutive runs and
/// word/path boundaries. Higher is better; `None` means no match.
pub fn fuzzy_score(query: &str, cand: &str) -> Option<i64> {
    let q: Vec<char> = query.to_lowercase().chars().collect();
    if q.is_empty() {
        // Empty query: everything matches; shorter paths first.
        return Some(-(cand.len() as i64));
    }
    let c: Vec<char> = cand.to_lowercase().chars().collect();
    let mut qi = 0usize;
    let mut score = 0i64;
    let mut last: i64 = -2;
    for (i, &ch) in c.iter().enumerate() {
        if qi < q.len() && ch == q[qi] {
            score += 1;
            if i as i64 == last + 1 {
                score += 3;
            }
            let boundary = i == 0 || matches!(c[i - 1], '/' | '_' | '-' | '.' | ' ');
            if boundary {
                score += 5;
            }
            last = i as i64;
            qi += 1;
        }
    }
    if qi == q.len() {
        // Prefer matches in the file name portion and shorter paths overall.
        let fname_start = cand.rfind('/').map(|i| i + 1).unwrap_or(0);
        if last as usize >= fname_start {
            score += 4;
        }
        Some(score - (c.len() as i64) / 8)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fuzzy_basics() {
        assert!(fuzzy_score("mn", "src/main.rs").is_some());
        assert!(fuzzy_score("xyz", "src/main.rs").is_none());
        assert!(fuzzy_score("", "anything").is_some());
        // filename match should beat a scattered path match
        let a = fuzzy_score("main", "src/main.rs").unwrap();
        let b = fuzzy_score("main", "media/art/insignia.txt").unwrap();
        assert!(a > b, "{a} vs {b}");
        // case-insensitive
        assert!(fuzzy_score("MAIN", "src/main.rs").is_some());
    }
}
