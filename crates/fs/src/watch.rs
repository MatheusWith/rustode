//! Watching files for changes made by other programs.
//!
//! Each watched file is observed through its parent directory, so the watch
//! survives the delete-and-recreate or write-and-rename that many programs
//! (rustode's own [`write_file`](crate::write_file) included) use to save.
//! The watcher only says *which* paths may have changed; callers compare the
//! bytes on disk to decide whether anything really did.

use notify::event::{EventKind, ModifyKind, RenameMode};
use notify::{RecommendedWatcher, RecursiveMode, Watcher as _};
use std::collections::HashMap;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// How long the first half of a rename waits for its second half before it
/// is reported as a removal (a file moved out of the watched directory).
///
/// VS Code waits the same 100 ms before believing a delete event
/// (`src/vs/workbench/services/textfile/common/textFileEditorModel.ts:187`).
const RENAME_GRACE: Duration = Duration::from_millis(100);

/// A change the watcher saw.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FsChange {
    /// The path was created, written or removed; read it to know which.
    Changed(PathBuf),
    /// The file at `from` now lives at `to`.
    Renamed {
        /// The old path.
        from: PathBuf,
        /// The new path.
        to: PathBuf,
    },
    /// Events were lost (the kernel queue overflowed): recheck every file.
    Rescan,
}

/// Watches files, reporting changes when polled. Paths are made absolute
/// (not canonical) and reported in that form.
pub struct Watcher {
    inner: RecommendedWatcher,
    events: mpsc::Receiver<notify::Result<notify::Event>>,
    /// Watched directories and how many watched files each holds.
    dirs: HashMap<PathBuf, usize>,
    /// First halves of renames, by tracker, waiting for their second half.
    pending: Vec<(usize, PathBuf, Instant)>,
}

impl fmt::Debug for Watcher {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Watcher")
            .field("dirs", &self.dirs)
            .field("pending", &self.pending)
            .finish_non_exhaustive()
    }
}

impl Watcher {
    /// A watcher with nothing watched yet.
    ///
    /// # Errors
    /// When the platform watcher cannot be created (e.g. the inotify
    /// instance limit is reached).
    pub fn new() -> io::Result<Self> {
        let (sender, events) = mpsc::channel();
        let inner = notify::recommended_watcher(sender).map_err(to_io)?;
        Ok(Self {
            inner,
            events,
            dirs: HashMap::new(),
            pending: Vec::new(),
        })
    }

    /// Starts watching `path`. Watching the same file twice is counted, and
    /// needs two [`unwatch_file`](Self::unwatch_file) calls to stop.
    ///
    /// # Errors
    /// When the file's directory cannot be watched.
    pub fn watch_file(&mut self, path: &Path) -> io::Result<()> {
        let dir = parent_dir(path)?;
        if let Some(count) = self.dirs.get_mut(&dir) {
            *count += 1;
            return Ok(());
        }
        self.inner
            .watch(&dir, RecursiveMode::NonRecursive)
            .map_err(to_io)?;
        tracing::debug!(dir = %dir.display(), "watching");
        self.dirs.insert(dir, 1);
        Ok(())
    }

    /// Stops watching `path`.
    ///
    /// # Errors
    /// When the platform watcher refuses to drop the directory watch.
    pub fn unwatch_file(&mut self, path: &Path) -> io::Result<()> {
        let dir = parent_dir(path)?;
        match self.dirs.get_mut(&dir) {
            Some(count) if *count > 1 => *count -= 1,
            Some(_) => {
                self.dirs.remove(&dir);
                self.inner.unwatch(&dir).map_err(to_io)?;
            }
            None => {}
        }
        Ok(())
    }

    /// The changes seen since the last poll, without blocking. Paths in
    /// watched directories that are not watched files are reported too.
    pub fn poll(&mut self) -> Vec<FsChange> {
        let mut changes = Vec::new();
        while let Ok(event) = self.events.try_recv() {
            match event {
                Ok(event) => self.translate(&event, &mut changes),
                Err(err) => {
                    tracing::warn!(%err, "file watcher error");
                    changes.push(FsChange::Rescan);
                }
            }
        }
        let now = Instant::now();
        self.pending.retain(|(_, from, since)| {
            let expired = now.duration_since(*since) >= RENAME_GRACE;
            if expired {
                changes.push(FsChange::Changed(from.clone()));
            }
            !expired
        });
        changes
    }

    fn translate(&mut self, event: &notify::Event, changes: &mut Vec<FsChange>) {
        if event.need_rescan() {
            changes.push(FsChange::Rescan);
        }
        let tracker = event.tracker();
        match (event.kind, tracker, event.paths.as_slice()) {
            (EventKind::Modify(ModifyKind::Name(RenameMode::From)), Some(tracker), [from]) => {
                self.pending.push((tracker, from.clone(), Instant::now()));
            }
            (EventKind::Modify(ModifyKind::Name(RenameMode::Both)), _, [from, to]) => {
                if let Some(tracker) = tracker {
                    self.pending.retain(|(pending, _, _)| *pending != tracker);
                }
                changes.push(FsChange::Renamed {
                    from: from.clone(),
                    to: to.clone(),
                });
            }
            (EventKind::Access(_), _, _) => {}
            (_, _, paths) => changes.extend(paths.iter().cloned().map(FsChange::Changed)),
        }
    }
}

fn parent_dir(path: &Path) -> io::Result<PathBuf> {
    let absolute = std::path::absolute(path)?;
    absolute
        .parent()
        .map(Path::to_owned)
        .ok_or_else(|| io::Error::other("the path has no parent directory"))
}

fn to_io(err: notify::Error) -> io::Error {
    match err.kind {
        notify::ErrorKind::Io(err) => err,
        _ => io::Error::other(err.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wait_for(watcher: &mut Watcher, want: impl Fn(&FsChange) -> bool) -> Vec<FsChange> {
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut seen = Vec::new();
        while Instant::now() < deadline {
            seen.extend(watcher.poll());
            if seen.iter().any(&want) {
                return seen;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("no matching change; saw {seen:?}");
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rustode-watch-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    #[test]
    fn a_rename_is_paired_and_a_move_out_becomes_a_change() {
        let dir = temp_dir("rename");
        let a = dir.join("a");
        std::fs::write(&a, b"x").expect("fixture");
        let mut watcher = Watcher::new().expect("watcher");
        watcher.watch_file(&a).expect("watch");
        let b = dir.join("b");
        std::fs::rename(&a, &b).expect("rename");
        let seen = wait_for(&mut watcher, |c| matches!(c, FsChange::Renamed { .. }));
        assert!(seen.contains(&FsChange::Renamed {
            from: a.clone(),
            to: b.clone()
        }));
        let outside = temp_dir("rename-outside").join("c");
        std::fs::rename(&b, &outside).expect("move out");
        wait_for(&mut watcher, |c| *c == FsChange::Changed(b.clone()));
    }

    #[test]
    fn watches_are_counted_per_directory() {
        let dir = temp_dir("count");
        let a = dir.join("a");
        let mut watcher = Watcher::new().expect("watcher");
        watcher.watch_file(&a).expect("watch");
        watcher.watch_file(&dir.join("b")).expect("watch");
        watcher.unwatch_file(&a).expect("unwatch");
        std::fs::write(&a, b"x").expect("write");
        wait_for(&mut watcher, |c| *c == FsChange::Changed(a.clone()));
    }
}
