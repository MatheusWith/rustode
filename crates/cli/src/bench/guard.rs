//! Guards that keep benchmark runs within the machine's limits and away
//! from the evidence: one App instance at a time and enough free memory
//! (AGENTS.md §3.4), and nothing written into folders 01 to 08 (§0.1).

use std::fs::{File, OpenOptions, TryLockError};
use std::path::{Component, Path, PathBuf};
use std::process::Command;

use super::{Failure, display};

/// The App is not started below this much `MemAvailable`.
pub(crate) const MIN_AVAILABLE_MB: u64 = 2_000;
/// The QA watchdog aborts the App below this much `MemAvailable`, as the QA
/// harness's scenarios do (`run_scenario.sh`: `--abort-below-mb 700`).
pub(crate) const ABORT_BELOW_MB: u64 = 700;
/// The QA watchdog also aborts when memory pressure (PSI `some avg10`) stays
/// above this for 5 samples, as `run_scenario.sh` does (`--abort-psi 40`).
pub(crate) const ABORT_PSI: u32 = 40;
/// A copy of the App bigger than this never goes into a RAM file system.
const MAX_COPY_IN_RAM_BYTES: u64 = 64 << 20;

/// An exclusive lock, held until dropped or the process ends.
#[derive(Debug)]
pub(crate) struct Lock {
    _file: File,
}

/// Takes the bench lock at `path` without waiting.
pub(crate) fn lock(path: &Path) -> Result<Lock, Failure> {
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(path)
        .map_err(|err| {
            Failure::Run(format!(
                "cannot open the lock {}: {err}",
                display::path(path)
            ))
        })?;
    match file.try_lock() {
        Ok(()) => Ok(Lock { _file: file }),
        Err(TryLockError::WouldBlock) => Err(Failure::Refused(format!(
            "another rustode bench holds {} (one App at a time)",
            display::path(path)
        ))),
        Err(TryLockError::Error(err)) => Err(Failure::Run(format!(
            "cannot lock {}: {err}",
            display::path(path)
        ))),
    }
}

/// `MemAvailable` in MB, from `<proc>/meminfo`.
pub(crate) fn mem_available_mb(proc_dir: &Path) -> Result<u64, String> {
    meminfo_mb(proc_dir, "MemAvailable:")
}

/// A `<proc>/meminfo` field in MB.
pub(crate) fn meminfo_mb(proc_dir: &Path, field: &str) -> Result<u64, String> {
    let path = proc_dir.join("meminfo");
    let text = std::fs::read_to_string(&path)
        .map_err(|err| format!("cannot read {}: {err}", path.display()))?;
    text.lines()
        .find_map(|line| line.strip_prefix(field))
        .and_then(|rest| rest.split_whitespace().next()?.parse::<u64>().ok())
        .map(|kb| kb / 1024)
        .ok_or_else(|| format!("no {field} in {}", path.display()))
}

/// Refuses below [`MIN_AVAILABLE_MB`]; returns the value read.
pub(crate) fn enough_memory(proc_dir: &Path) -> Result<u64, Failure> {
    let available = mem_available_mb(proc_dir).map_err(Failure::Refused)?;
    if available < MIN_AVAILABLE_MB {
        return Err(Failure::Refused(format!(
            "MemAvailable {available} MB is below {MIN_AVAILABLE_MB} MB, the minimum to start the App (AGENTS.md §3.4)"
        )));
    }
    Ok(available)
}

/// Refuses while any App (`codium`) process runs: one instance at a time.
pub(crate) fn no_app_running(proc_dir: &Path) -> Result<(), Failure> {
    let running = app_processes(proc_dir);
    if running.is_empty() {
        return Ok(());
    }
    let list: Vec<String> = running
        .iter()
        .take(5)
        .map(|(pid, exe)| format!("pid {pid} {exe}"))
        .collect();
    Err(Failure::Refused(format!(
        "one App at a time: {} App process(es) already running ({}); close it first",
        running.len(),
        list.join(", ")
    )))
}

/// Processes whose executable or first argument is a `codium`, or lives in
/// a `VSCode-linux-x64` folder: the App, a copy of it, or a `VSCodium`.
fn app_processes(proc_dir: &Path) -> Vec<(u32, String)> {
    let Ok(entries) = std::fs::read_dir(proc_dir) else {
        return Vec::new();
    };
    let me = std::process::id();
    let mut found: Vec<(u32, String)> = entries
        .filter_map(Result::ok)
        .filter_map(|e| Some((e.file_name().to_str()?.parse::<u32>().ok()?, e.path())))
        .filter(|(pid, _)| *pid != me)
        .filter_map(|(pid, dir)| {
            let exe = std::fs::read_link(dir.join("exe"))
                .ok()
                .map(|p| p.to_string_lossy().into_owned());
            let argv0 = std::fs::read(dir.join("cmdline")).ok().and_then(|bytes| {
                let first = bytes.split(|b| *b == 0).next()?;
                Some(String::from_utf8_lossy(first).into_owned())
            });
            [exe, argv0]
                .into_iter()
                .flatten()
                .find(|p| is_app(p))
                .map(|p| (pid, p))
        })
        .collect();
    found.sort();
    found
}

fn is_app(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    name == "codium" || path.contains("/VSCode-linux-x64/")
}

/// Refuses a copy of `bytes` bytes into a RAM file system (`tmpfs`, `ramfs`):
/// on this machine `/tmp` is a `tmpfs`, and the App is about 700 MB.
pub(crate) fn copy_fits(dir: &Path, bytes: u64) -> Result<(), Failure> {
    let existing = dir.ancestors().find(|p| p.exists()).unwrap_or(dir);
    let fs_type = Command::new("stat")
        .args(["-f", "-c", "%T"])
        .arg(existing)
        .output()
        .ok()
        .filter(|out| out.status.success())
        .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_owned());
    let in_ram = matches!(fs_type.as_deref(), Some("tmpfs" | "ramfs"));
    if in_ram && bytes > MAX_COPY_IN_RAM_BYTES {
        return Err(Failure::Refused(format!(
            "the App copy ({} MB) would go to {}, a RAM file system; pass --app-copy-dir with a folder on disk",
            bytes >> 20,
            display::path(existing)
        )));
    }
    Ok(())
}

/// The evidence folder (`01-…` to `08-…`) of `project` that contains `path`.
pub(crate) fn evidence_folder(project: &Path, path: &Path) -> Option<String> {
    let project = resolve(project);
    let rest = resolve(path).strip_prefix(&project).ok()?.to_owned();
    let first = rest
        .components()
        .next()?
        .as_os_str()
        .to_string_lossy()
        .into_owned();
    let bytes = first.as_bytes();
    let evidence = bytes.len() > 2 && bytes[0] == b'0' && (b'1'..=b'8').contains(&bytes[1]);
    evidence.then_some(first)
}

/// `path` made absolute and resolved through its longest existing ancestor,
/// so paths that do not exist yet compare correctly.
pub(crate) fn resolve(path: &Path) -> PathBuf {
    let absolute = std::path::absolute(path).unwrap_or_else(|_| path.to_owned());
    let mut normal = PathBuf::new();
    for part in absolute.components() {
        match part {
            Component::ParentDir => {
                normal.pop();
            }
            Component::CurDir => {}
            other => normal.push(other),
        }
    }
    let mut existing = normal.as_path();
    let mut missing = Vec::new();
    while !existing.exists() {
        let (Some(parent), Some(name)) = (existing.parent(), existing.file_name()) else {
            return normal;
        };
        missing.push(name.to_owned());
        existing = parent;
    }
    let mut resolved = existing
        .canonicalize()
        .unwrap_or_else(|_| existing.to_owned());
    resolved.extend(missing.iter().rev());
    resolved
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_paths_are_recognized() {
        assert!(is_app("/opt/vscodium-bin/codium"));
        assert!(is_app("/x/VSCode-linux-x64/chrome_crashpad_handler"));
        assert!(!is_app("/usr/bin/rustode"));
        assert!(!is_app("/usr/bin/codium-helper"));
    }

    #[test]
    fn a_big_copy_is_refused_on_a_ram_file_system() {
        // /dev/shm is a tmpfs on Linux; the folder need not exist yet.
        let shm = Path::new("/dev/shm/rustode-bench-test/copy");
        let refused = copy_fits(shm, 700 << 20).expect_err("700 MB into tmpfs");
        assert!(refused.to_string().contains("RAM file system"), "{refused}");
        assert!(
            copy_fits(shm, 1 << 20).is_ok(),
            "small copies may go to RAM"
        );
        let disk = Path::new(env!("CARGO_MANIFEST_DIR"));
        let disk_is_ram = Command::new("stat")
            .args(["-f", "-c", "%T"])
            .arg(disk)
            .output()
            .is_ok_and(|out| out.stdout.starts_with(b"tmpfs"));
        if !disk_is_ram {
            assert!(copy_fits(disk, 700 << 20).is_ok());
        }
    }

    #[test]
    fn evidence_folders_are_found_through_dot_dot() {
        let project = Path::new("/nonexistent/project");
        let inside = Path::new("/nonexistent/project/target/../01-app/x");
        assert_eq!(evidence_folder(project, inside).as_deref(), Some("01-app"));
        assert_eq!(
            evidence_folder(project, Path::new("/nonexistent/project/09-analises")),
            None
        );
        assert_eq!(
            evidence_folder(project, Path::new("/elsewhere/01-app")),
            None
        );
    }
}
