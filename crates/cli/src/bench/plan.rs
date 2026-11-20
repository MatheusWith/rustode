//! Everything a run will do, resolved before anything starts: paths, the
//! exact commands and the guards. `--dry-run` prints it; a run executes it.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use super::args::{Options, Side};
use super::guard::{ABORT_BELOW_MB, ABORT_PSI, MIN_AVAILABLE_MB};
use super::scenario::{Cache, GuiSide, Kind, List, Scenario};
use super::{Failure, display, fixture, guard};

/// The CDP port of the App; inside its own network namespace, so it never
/// meets another process's port.
pub(crate) const CDP_PORT: u16 = 9333;
/// The memory cap of the QA launcher (`launch_code.sh`).
const APP_SCOPE: [&str; 9] = [
    "systemd-run",
    "--user",
    "--scope",
    "--quiet",
    "-p",
    "MemoryMax=3G",
    "-p",
    "MemorySwapMax=1G",
    "--",
];
/// Name of the App folder in `01-app/` and in each copy.
pub(crate) const APP_FOLDER: &str = "VSCode-linux-x64";

/// A resolved run.
#[derive(Debug)]
pub(crate) struct Plan<'a> {
    /// The scenario list.
    pub list: &'a List,
    /// The scenario to run.
    pub scenario: &'a Scenario,
    /// Which sides run.
    pub side: Side,
    /// When the plan was made.
    pub started: SystemTime,
    /// `<UTC stamp>-<scenario id>`; failed runs get a suffix later.
    pub run_id: String,
    /// The command line, as typed.
    pub argv: Vec<String>,
    /// The folder the command ran from.
    pub cwd: PathBuf,
    /// `<out>/<run id>`.
    pub run_folder: PathBuf,
    /// The rustode project with `01-app/` and `02-package-analysis/`.
    pub project: PathBuf,
    /// Where the App copy folder is created.
    pub copy_parent: PathBuf,
    /// The QA harness (`~/Projects/vscode/qa`).
    pub qa_root: PathBuf,
    /// Node for the App driver.
    pub node: PathBuf,
    /// The bench lock.
    pub lock: PathBuf,
    /// `/proc`, or a stand-in for tests (`RUSTODE_BENCH_PROC`).
    pub proc_dir: PathBuf,
    /// The rustode GUI binary for launch scenarios.
    pub gui_bin: PathBuf,
    /// Mesa's EGL vendor file: makes the GUI's GL use Mesa's llvmpipe instead
    /// of the machine's GPU driver (`RUSTODE_BENCH_MESA_EGL`).
    pub mesa_egl: PathBuf,
}

/// `rustode-gui` in the folder of the running `rustode`.
fn default_gui_bin() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join("rustode-gui")))
        .unwrap_or_else(|| PathBuf::from("rustode-gui"))
}

impl<'a> Plan<'a> {
    /// Resolves `options` for `scenario`; refuses folders inside the evidence.
    pub(crate) fn new(
        options: &Options,
        list: &'a List,
        scenario: &'a Scenario,
    ) -> Result<Self, Failure> {
        let cwd = std::env::current_dir()
            .map_err(|err| Failure::Run(format!("no current folder: {err}")))?;
        let started = SystemTime::now();
        let run_id = format!("{}-{}", display::run_stamp(started), scenario.id);
        let run_folder = guard::resolve(&options.out.join(&run_id));
        let project = guard::resolve(&options.project);
        let copy_parent = options
            .app_copy_dir
            .as_deref()
            .map_or_else(|| run_folder.clone(), guard::resolve);
        for (what, path) in [("--out", &run_folder), ("--app-copy-dir", &copy_parent)] {
            if let Some(folder) = guard::evidence_folder(&project, path) {
                return Err(Failure::Usage(format!(
                    "{what} {} is inside the evidence folder {folder}, which is read-only (AGENTS.md §0.1)",
                    display::path(path)
                )));
            }
        }
        let home = std::env::var_os("HOME").map_or_else(Default::default, PathBuf::from);
        let env_path =
            |name: &str, default: PathBuf| std::env::var_os(name).map_or(default, PathBuf::from);
        let runtime =
            std::env::var_os("XDG_RUNTIME_DIR").map_or_else(std::env::temp_dir, PathBuf::from);
        Ok(Self {
            list,
            scenario,
            side: options.side,
            started,
            run_id,
            argv: std::env::args().collect(),
            cwd,
            run_folder,
            project,
            copy_parent,
            qa_root: env_path("RUSTODE_QA_ROOT", home.join("Projects/vscode/qa")),
            node: env_path(
                "RUSTODE_NODE",
                home.join(".local/opt/node-v24.22.1-linux-x64/bin/node"),
            ),
            lock: env_path("RUSTODE_BENCH_LOCK", runtime.join("rustode-bench.lock")),
            proc_dir: env_path("RUSTODE_BENCH_PROC", PathBuf::from("/proc")),
            mesa_egl: env_path(
                "RUSTODE_BENCH_MESA_EGL",
                PathBuf::from("/usr/share/glvnd/egl_vendor.d/50_mesa.json"),
            ),
            gui_bin: options
                .gui_bin
                .as_deref()
                .map_or_else(default_gui_bin, guard::resolve),
        })
    }

    /// Whether rustode's in-process side runs (`open-file` scenarios).
    pub(crate) fn runs_in_process(&self) -> bool {
        self.runs_rustode() && self.scenario.kind == Kind::OpenFile
    }

    /// Whether the rustode GUI is launched.
    pub(crate) fn runs_gui(&self) -> bool {
        self.runs_rustode() && self.scenario.gui.is_some()
    }

    /// Whether the rustode side runs.
    pub(crate) fn runs_rustode(&self) -> bool {
        self.side != Side::App
    }

    /// Whether the App side runs.
    pub(crate) fn runs_app(&self) -> bool {
        self.side != Side::Rustode
    }

    /// The App under analysis, `01-app/VSCode-linux-x64`.
    pub(crate) fn app_dir(&self) -> PathBuf {
        self.project.join("01-app").join(APP_FOLDER)
    }

    /// The SHA-256 manifest of the App.
    pub(crate) fn manifest(&self) -> PathBuf {
        self.project.join("02-package-analysis/manifest-sha256.txt")
    }

    /// `codium --version` of the App, recorded by `assemble.sh`.
    pub(crate) fn version_file(&self) -> PathBuf {
        self.project.join("02-package-analysis/version.txt")
    }

    /// The generated fixture.
    pub(crate) fn fixture(&self) -> PathBuf {
        self.run_folder
            .join("fixtures")
            .join(format!("{}.txt", self.scenario.id))
    }

    /// The fixture's first line, which the App must show.
    pub(crate) fn first_line(&self) -> String {
        fixture::line(1, self.scenario.fixture.line_bytes)
    }

    /// The App driver script.
    pub(crate) fn driver(&self) -> PathBuf {
        self.list_dir().join(&self.scenario.app.driver)
    }

    /// The `settings.json` for each profile.
    pub(crate) fn settings(&self) -> PathBuf {
        self.list_dir().join(&self.scenario.app.settings)
    }

    fn list_dir(&self) -> PathBuf {
        guard::resolve(self.list.path.parent().unwrap_or_else(|| Path::new(".")))
    }

    /// The QA harness's virtual-display runner.
    pub(crate) fn run_virtual(&self) -> PathBuf {
        self.qa_root.join("suite/tools/run_virtual.sh")
    }

    /// The QA harness's resource sampler and memory watchdog.
    pub(crate) fn sampler(&self) -> PathBuf {
        self.qa_root.join("suite/collectors/sampler.py")
    }

    /// The QA harness's CDP helpers.
    pub(crate) fn qa_lib(&self) -> PathBuf {
        self.qa_root.join("suite/scenarios/lib.cjs")
    }

    /// The `playwright-core` the QA harness uses (`lib.cjs`).
    pub(crate) fn playwright_core(&self) -> PathBuf {
        self.qa_root.join("../node_modules/playwright-core")
    }

    /// Where the driver writes its results and logs.
    pub(crate) fn app_out(&self) -> PathBuf {
        self.run_folder.join("app")
    }

    /// The driver configuration file.
    pub(crate) fn driver_config(&self) -> PathBuf {
        self.app_out().join("driver-config.json")
    }

    /// Where the GUI driver writes its results and logs.
    pub(crate) fn gui_out(&self) -> PathBuf {
        self.run_folder.join("gui")
    }

    /// The GUI driver configuration file.
    pub(crate) fn gui_driver_config(&self) -> PathBuf {
        self.gui_out().join("driver-config.json")
    }

    /// The page-cache eviction helper.
    pub(crate) fn evict_script(&self) -> PathBuf {
        self.list_dir().join("evict.py")
    }

    /// One launch of the rustode GUI with the fixture. `{profile}` is filled
    /// in by the driver.
    pub(crate) fn gui_command(&self) -> Vec<String> {
        vec![
            self.gui_bin.to_string_lossy().into_owned(),
            self.fixture().to_string_lossy().into_owned(),
        ]
    }

    /// Software rendering for either editor: the virtual display has no GPU,
    /// this machine has no Vulkan software driver (lavapipe), and its default
    /// GL driver is the NVIDIA one, so both editors are pointed at Mesa's
    /// llvmpipe through GL: the vendor file keeps the NVIDIA EGL driver out and
    /// the empty Vulkan ICD list keeps its Vulkan driver out.
    pub(crate) fn software_rendering_env(&self) -> Vec<(String, String)> {
        let kv = |k: &str, v: &str| (k.to_owned(), v.to_owned());
        vec![
            kv("LIBGL_ALWAYS_SOFTWARE", "1"),
            kv("GALLIUM_DRIVER", "llvmpipe"),
            kv(
                "__EGL_VENDOR_LIBRARY_FILENAMES",
                &self.mesa_egl.to_string_lossy(),
            ),
            kv("__GLX_VENDOR_LIBRARY_NAME", "mesa"),
            kv("VK_ICD_FILENAMES", "/nonexistent"),
        ]
    }

    /// The environment of a GUI launch: its own profile folders (never the
    /// user's), the first-frame report, wgpu's GL backend and software
    /// rendering.
    pub(crate) fn gui_env(&self) -> Vec<(String, String)> {
        let kv = |k: &str, v: &str| (k.to_owned(), v.to_owned());
        let mut env = vec![
            kv("RUSTODE_REPORT_FIRST_FRAME", "1"),
            kv("RUSTODE_USER_DIR", "{profile}/user"),
            kv("XDG_CONFIG_HOME", "{profile}/config"),
            kv("XDG_DATA_HOME", "{profile}/data"),
            kv("XDG_STATE_HOME", "{profile}/state"),
            kv("XDG_CACHE_HOME", "{profile}/cache"),
            kv("WGPU_BACKEND", "gl"),
        ];
        env.extend(self.software_rendering_env());
        env
    }

    /// The watchdog for one GUI launch: by process id, `{pid}` filled in by
    /// the driver.
    pub(crate) fn gui_sampler_command(&self) -> Vec<String> {
        let mut command = self.sampler_command();
        if let Some(i) = command.iter().position(|a| a == "--match") {
            "--pid".clone_into(&mut command[i]);
            "{pid}".clone_into(&mut command[i + 1]);
        }
        command
    }

    /// The cache state of the scenario's launches.
    pub(crate) fn cache(&self) -> Cache {
        self.scenario.launch.map_or(Cache::Warm, |l| l.cache)
    }

    /// One launch of the App: the QA launcher's memory cap, an isolated
    /// profile, CDP, and the fixture on the command line. `{profile}` is
    /// filled in by the driver for each launch.
    pub(crate) fn app_command(&self, codium: &Path) -> Vec<String> {
        let mut command: Vec<String> = APP_SCOPE.iter().map(|s| (*s).to_owned()).collect();
        command.extend([
            codium.to_string_lossy().into_owned(),
            "--user-data-dir={profile}/ud".to_owned(),
            "--extensions-dir={profile}/ext".to_owned(),
            "--shared-data-dir={profile}/shared".to_owned(),
            "--disable-telemetry".to_owned(),
            "--disable-workspace-trust".to_owned(),
            format!("--remote-debugging-port={CDP_PORT}"),
            "--new-window".to_owned(),
            self.fixture().to_string_lossy().into_owned(),
        ]);
        command
    }

    /// The QA watchdog for one launch, as `run_scenario.sh` runs it.
    pub(crate) fn sampler_command(&self) -> Vec<String> {
        [
            "python3",
            "-I",
            &self.sampler().to_string_lossy(),
            "--match",
            "{profile_re}/ud",
            "--interval",
            "1",
            "--out",
            "{rep_dir}/sampler.jsonl",
            "--abort-below-mb",
            &ABORT_BELOW_MB.to_string(),
            "--abort-psi",
            &ABORT_PSI.to_string(),
        ]
        .map(str::to_owned)
        .to_vec()
    }

    /// The driver on the QA harness's virtual display, without network.
    pub(crate) fn launch_command(&self, config: &Path) -> Vec<String> {
        let mut command = vec![self.run_virtual().to_string_lossy().into_owned()];
        command.extend(
            [
                "bwrap",
                "--dev-bind",
                "/",
                "/",
                "--unshare-net",
                "--die-with-parent",
            ]
            .map(str::to_owned),
        );
        command.extend(
            [&self.node, &self.driver(), &config.to_path_buf()]
                .map(|p| p.to_string_lossy().into_owned()),
        );
        command
    }

    /// The longest the whole App phase may take before it is killed.
    pub(crate) fn app_deadline(&self) -> Duration {
        let app = &self.scenario.app;
        let launches = u64::from(app.warmup + app.repetitions);
        Duration::from_secs(launches * (u64::from(app.timeout_s) + self.settle_s() + 20) + 60)
    }

    /// The longest the GUI phase may take before it is killed.
    pub(crate) fn gui_deadline(&self) -> Duration {
        let gui = self.scenario.gui.unwrap_or(GuiSide {
            warmup: 0,
            repetitions: 0,
            timeout_s: 0,
        });
        let launches = u64::from(gui.warmup + gui.repetitions);
        Duration::from_secs(launches * (u64::from(gui.timeout_s) + self.settle_s() + 20) + 60)
    }

    fn settle_s(&self) -> u64 {
        self.scenario.launch.map_or(0, |l| u64::from(l.settle_s))
    }

    /// The `--dry-run` output.
    pub(crate) fn describe(&self) -> String {
        let mut out = String::from("rustode bench: dry run: nothing is launched or written\n\n");
        let _ = writeln!(out, "{:<12}{}", "run", self.run_id);
        let _ = writeln!(out, "{:<12}{}", "command", display::command(&self.argv));
        let _ = writeln!(out, "{:<12}{}", "folder", display::path(&self.cwd));
        let _ = writeln!(
            out,
            "{:<12}{}",
            "run folder",
            display::path(&self.run_folder)
        );
        let _ = writeln!(out, "{:<12}{}\n", "scenario", self.scenario_line());
        let _ = writeln!(out, "plan");
        self.describe_steps(&mut out);
        out
    }

    /// `<id>: <title> (<list>, list version <n>)`.
    pub(crate) fn scenario_line(&self) -> String {
        format!(
            "{}: {} ({}, list version {})",
            self.scenario.id,
            self.scenario.title,
            display::path(&self.list.path),
            self.list.version
        )
    }

    fn describe_steps(&self, out: &mut String) {
        let s = self.scenario;
        let _ = writeln!(
            out,
            "  1. fixture: generate {} ({} bytes, numbered ASCII lines of {} bytes with LF)",
            display::path(&self.fixture()),
            s.fixture.bytes,
            s.fixture.line_bytes
        );
        if self.runs_in_process() {
            let build = if cfg!(debug_assertions) {
                "debug"
            } else {
                "release"
            };
            let _ = writeln!(
                out,
                "  2. rustode: Workspace::open in this process, {} warm-up + {} recorded ({build} build)",
                s.rustode.warmup, s.rustode.repetitions
            );
        }
        if self.runs_gui() {
            self.describe_gui(out);
        }
        if self.runs_app() {
            self.describe_app(out);
        }
        let _ = writeln!(
            out,
            "  7. report: {} and report.json",
            display::path(&self.run_folder.join("report.txt"))
        );
    }

    fn describe_gui(&self, out: &mut String) {
        let Some(gui) = self.scenario.gui else {
            return;
        };
        let _ = writeln!(
            out,
            "  2b. rustode GUI: {} on the virtual display, without network, {} warm-up + {} recorded, timeout {} s each:",
            display::path(&self.gui_bin),
            gui.warmup,
            gui.repetitions,
            gui.timeout_s
        );
        let _ = writeln!(
            out,
            "       {}",
            display::command(&self.launch_command(&self.gui_driver_config()))
        );
        let _ = writeln!(
            out,
            "       launch: {}",
            display::command(&self.gui_command())
        );
        let env: Vec<String> = self
            .gui_env()
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect();
        let _ = writeln!(out, "       environment: {}", env.join(" "));
        if let Some(launch) = self.scenario.launch {
            let _ = writeln!(
                out,
                "       measures {}; file cache {}{}",
                launch.measure.as_str(),
                launch.cache.as_str(),
                if launch.settle_s > 0 {
                    format!("; memory read {} s after the first frame", launch.settle_s)
                } else {
                    String::new()
                }
            );
        }
    }

    fn describe_app(&self, out: &mut String) {
        let app = &self.scenario.app;
        let (app_dir, copy) = (
            self.app_dir(),
            self.copy_parent.join("rustode-bench-app.XXXXXXXXXXXXXXXX"),
        );
        let codium = copy.join(APP_FOLDER).join("codium");
        let lines = [
            format!(
                "  3. App integrity: `sha256sum -c --quiet {}` run in {} must print nothing, and every file there must be listed (refuse if any file is not in the manifest)",
                display::path(&self.manifest()),
                display::path(&app_dir)
            ),
            format!(
                "     one App at a time: lock {}, refuse if a codium process runs; refuse below {MIN_AVAILABLE_MB} MB MemAvailable",
                display::path(&self.lock)
            ),
            format!(
                "  4. App copy: cp -a {} {}, then check the copy the same way",
                display::path(&app_dir),
                display::path(&copy.join(APP_FOLDER))
            ),
            format!(
                "  5. App launches: {}",
                display::command(&self.launch_command(&self.driver_config()))
            ),
            format!(
                "     {} warm-up + {} recorded; each with a fresh profile {{profile}} in {}:",
                app.warmup,
                app.repetitions,
                display::path(&copy.join("profiles"))
            ),
            format!("       {}", display::command(&self.app_command(&codium))),
            format!(
                "     watchdog: {}",
                display::command(&self.sampler_command())
            ),
            format!(
                "     timeout {} s per launch; the editor is checked every {} ms for the fixture's first line",
                app.timeout_s, app.poll_ms
            ),
            format!(
                "  6. App cleanup: delete the copy, then check {} again",
                display::path(&app_dir)
            ),
        ];
        for line in lines {
            let _ = writeln!(out, "{line}");
        }
    }
}
