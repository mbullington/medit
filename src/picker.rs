use std::{ffi::OsStr, path::PathBuf, time::Instant};

use ignore::WalkBuilder;
use nucleo::{
    Config as NucleoConfig, Matcher, Utf32Str,
    pattern::{CaseMatching, Normalization, Pattern},
};

#[derive(Debug)]
pub struct Picker {
    pub active: bool,
    pub query: String,
    cwd: PathBuf,
    files: Vec<PathBuf>,
    filtered: Vec<usize>,
    selected: usize,
    last_refresh: Instant,
}

impl Picker {
    pub fn new(cwd: PathBuf) -> Self {
        let mut picker = Self {
            active: false,
            query: String::new(),
            cwd,
            files: Vec::new(),
            filtered: Vec::new(),
            selected: 0,
            last_refresh: Instant::now(),
        };
        picker.refresh_files();
        picker
    }

    pub fn open(&mut self) {
        self.active = true;
        self.query.clear();
        self.selected = 0;
        if self.last_refresh.elapsed().as_secs() > 2 {
            self.refresh_files();
        }
        self.refilter();
    }

    pub fn close(&mut self) {
        self.active = false;
    }

    pub fn push(&mut self, ch: char) {
        self.query.push(ch);
        self.refilter();
    }

    pub fn pop(&mut self) {
        self.query.pop();
        self.refilter();
    }

    pub fn move_selection(&mut self, delta: isize) {
        if self.filtered.is_empty() {
            self.selected = 0;
            return;
        }
        let len = self.filtered.len() as isize;
        self.selected = (self.selected as isize + delta).clamp(0, len - 1) as usize;
    }

    pub fn page(&mut self, delta: isize) {
        self.move_selection(delta * 10);
    }

    pub fn selected_path(&self) -> Option<PathBuf> {
        self.filtered
            .get(self.selected)
            .and_then(|idx| self.files.get(*idx))
            .map(|p| self.cwd.join(p))
    }

    pub fn create_path(&self) -> Option<PathBuf> {
        let query = self.query.trim();
        if query.is_empty() {
            return None;
        }
        Some(self.cwd.join(query))
    }

    pub fn results(&self) -> impl Iterator<Item = (usize, &PathBuf)> {
        self.filtered
            .iter()
            .enumerate()
            .filter_map(|(row, idx)| self.files.get(*idx).map(|path| (row, path)))
    }

    pub fn selected(&self) -> usize {
        self.selected
    }

    pub fn cwd(&self) -> &PathBuf {
        &self.cwd
    }

    fn refresh_files(&mut self) {
        self.files.clear();
        let mut builder = WalkBuilder::new(&self.cwd);
        builder
            .hidden(false)
            .git_ignore(true)
            .git_global(true)
            .git_exclude(true)
            .parents(true)
            .filter_entry(|entry| entry.file_name() != OsStr::new(".git"));
        let walker = builder.build();
        for entry in walker.flatten() {
            let path = entry.path();
            if path == self.cwd || !path.is_file() {
                continue;
            }
            if let Ok(rel) = path.strip_prefix(&self.cwd) {
                if rel
                    .components()
                    .any(|component| component.as_os_str() == OsStr::new(".git"))
                {
                    continue;
                }
                self.files.push(rel.to_path_buf());
            }
        }
        self.files.sort();
        self.last_refresh = Instant::now();
        self.refilter();
    }

    fn refilter(&mut self) {
        let query = self.query.trim();
        let pattern = Pattern::parse(query, CaseMatching::Smart, Normalization::Smart);
        let mut matcher = Matcher::new(NucleoConfig::DEFAULT);
        let mut utf32 = Vec::new();
        let mut scored = Vec::new();
        for (idx, path) in self.files.iter().enumerate() {
            let s = path.to_string_lossy();
            if query.is_empty() {
                scored.push((0, idx));
            } else if let Some(score) = pattern.score(Utf32Str::new(&s, &mut utf32), &mut matcher) {
                scored.push((score as i64, idx));
            }
        }
        scored.sort_by(|a, b| {
            b.0.cmp(&a.0)
                .then_with(|| self.files[a.1].cmp(&self.files[b.1]))
        });
        self.filtered = scored.into_iter().map(|(_, idx)| idx).collect();
        self.selected = self.selected.min(self.filtered.len().saturating_sub(1));
    }
}
