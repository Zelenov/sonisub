//! Many inputs: folders expanded into media files, filters, and a plan of what each file needs
//! before anything is uploaded.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::audio;
use crate::job::{self, Options};
use crate::srt;

/// Extensions picked up from folders (explicitly named files are taken as they are).
pub const MEDIA_EXTENSIONS: &[&str] =
    &["mp4", "mov", "m4v", "mkv", "webm", "m4a", "mp3", "wav", "flac", "ogg", "opus", "aac"];

/// One file to process and where its subtitles go.
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub input: PathBuf,
    pub output: PathBuf,
    /// For display: path inside the scanned folder, or the file name.
    pub name: String,
}

/// Where inputs come from and how folders are read.
#[derive(Debug, Clone, Default)]
pub struct Selection {
    /// Wildcard patterns for file names in folders ("*.MP4", "Interview*"); any may match. Empty = all media.
    pub filters: Vec<String>,
    /// Descend into subfolders.
    pub recursive: bool,
    /// Put all outputs here (keeping subfolders of a scanned folder) instead of next to each input.
    pub out_dir: Option<PathBuf>,
}

/// Expands folders into their media files (sorted by name), keeps files as given, drops duplicates.
pub fn collect(inputs: &[PathBuf], sel: &Selection) -> Result<Vec<Item>> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for input in inputs {
        if input.is_dir() {
            let mut files = Vec::new();
            scan(input, sel, &mut files)?;
            files.sort_by_key(|p| p.to_string_lossy().to_lowercase());
            for f in files {
                let rel = f.strip_prefix(input).unwrap_or(&f).to_path_buf();
                let out_dir = sel.out_dir.as_ref().map(|d| d.join(rel.parent().unwrap_or(Path::new(""))));
                push(&mut out, &mut seen, &f, out_dir.as_deref(), rel.to_string_lossy().replace('\\', "/"));
            }
        } else {
            let name = input.file_name().map_or_else(|| input.display().to_string(), |n| n.to_string_lossy().into());
            push(&mut out, &mut seen, input, sel.out_dir.as_deref(), name);
        }
    }
    Ok(out)
}

fn push(out: &mut Vec<Item>, seen: &mut HashSet<PathBuf>, input: &Path, out_dir: Option<&Path>, name: String) {
    let key = input.canonicalize().unwrap_or_else(|_| input.to_path_buf());
    if seen.insert(key) {
        out.push(Item { input: input.to_path_buf(), output: job::default_output(input, out_dir), name });
    }
}

fn scan(dir: &Path, sel: &Selection, out: &mut Vec<PathBuf>) -> Result<()> {
    let entries = std::fs::read_dir(dir).with_context(|| format!("cannot read folder {}", dir.display()))?;
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        // Hidden files and macOS "._" resource forks next to camera files.
        if name.starts_with('.') {
            continue;
        }
        let Ok(kind) = entry.file_type() else { continue };
        if kind.is_dir() {
            if sel.recursive {
                scan(&path, sel, out)?;
            }
        } else if is_media(&path) && (sel.filters.is_empty() || sel.filters.iter().any(|f| matches(f, &name))) {
            out.push(path);
        }
    }
    Ok(())
}

pub fn is_media(p: &Path) -> bool {
    p.extension().and_then(|e| e.to_str()).is_some_and(|e| MEDIA_EXTENSIONS.contains(&e.to_lowercase().as_str()))
}

/// Case-insensitive wildcard match of a whole file name: `*` any run of characters, `?` one character.
/// A pattern without wildcards matches names containing it.
pub fn matches(pattern: &str, name: &str) -> bool {
    let p: Vec<char> = pattern.to_lowercase().chars().collect();
    let n: Vec<char> = name.to_lowercase().chars().collect();
    if !p.iter().any(|c| *c == '*' || *c == '?') {
        return name.to_lowercase().contains(&pattern.to_lowercase());
    }
    // Iterative glob matching with backtracking to the last '*'.
    let (mut i, mut j, mut star, mut mark) = (0, 0, None, 0);
    while j < n.len() {
        if i < p.len() && (p[i] == '?' || p[i] == n[j]) {
            i += 1;
            j += 1;
        } else if i < p.len() && p[i] == '*' {
            star = Some(i);
            mark = j;
            i += 1;
        } else if let Some(s) = star {
            i = s + 1;
            mark += 1;
            j = mark;
        } else {
            return false;
        }
    }
    while i < p.len() && p[i] == '*' {
        i += 1;
    }
    i == p.len()
}

/// What a file needs, decided from the file system and media headers only.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    /// Every output exists and `force` is off.
    Skip,
    /// The input is a transcript: subtitles are built from it.
    FromTranscript,
    /// A saved `.soniox.json` next to the output will be used: free.
    Cached { speech: bool },
    /// No audio track: nothing to do.
    NoAudio,
    /// Will be sent to Soniox; `audio_s` from the header when it could be read.
    Transcribe { audio_s: Option<f64> },
}

#[derive(Debug, Clone)]
pub struct Planned {
    pub item: Item,
    pub action: Action,
}

pub fn plan(items: &[Item], opts: &Options) -> Vec<Planned> {
    items.iter().map(|item| Planned { item: item.clone(), action: action(item, opts) }).collect()
}

fn action(item: &Item, opts: &Options) -> Action {
    if job::missing_outputs(&item.output, &opts.formats, opts.force).is_empty() {
        return Action::Skip;
    }
    if job::is_transcript(&item.input) {
        return Action::FromTranscript;
    }
    let cache = job::transcript_path(&item.output);
    if cache.is_file() && !opts.force {
        let speech = std::fs::read_to_string(&cache)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .is_some_and(|v| srt::has_speech(&v));
        return Action::Cached { speech };
    }
    match audio::probe(&item.input) {
        Ok(audio::Probe::NoAudio) => Action::NoAudio,
        Ok(audio::Probe::Audio(audio_s)) => Action::Transcribe { audio_s },
        // Unreadable header: let the real run decide (ffmpeg may still handle it).
        Err(_) => Action::Transcribe { audio_s: None },
    }
}

/// Counts and audio length per kind of action.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Totals {
    pub skip: usize,
    pub from_transcript: usize,
    pub cached: usize,
    pub cached_silent: usize,
    pub no_audio: usize,
    pub transcribe: usize,
    /// Audio to send, seconds (files with unknown length not included).
    pub audio_s: f64,
    /// Files to send whose length is unknown.
    pub unknown_length: usize,
}

impl Totals {
    pub fn of(plan: &[Planned]) -> Self {
        let mut t = Self::default();
        for p in plan {
            match p.action {
                Action::Skip => t.skip += 1,
                Action::FromTranscript => t.from_transcript += 1,
                Action::Cached { speech: true } => t.cached += 1,
                Action::Cached { speech: false } => t.cached_silent += 1,
                Action::NoAudio => t.no_audio += 1,
                Action::Transcribe { audio_s } => {
                    t.transcribe += 1;
                    match audio_s {
                        Some(s) => t.audio_s += s,
                        None => t.unknown_length += 1,
                    }
                }
            }
        }
        t
    }
}
