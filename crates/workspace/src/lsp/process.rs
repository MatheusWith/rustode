//! One language server process: its stdin fed by a writer thread, its stdout
//! read by a reader thread into a channel, its stderr logged. The workspace
//! never blocks on the server: it sends through a channel and drains the
//! incoming one when a frontend polls. The reader calls the [`Waker`] after
//! each message, so an idle frontend need not poll on a timer.

use super::rpc;
use super::settings::ServerConfig;
use rustode_trace::spans::{self, Direction};
use serde_json::{Value, json};
use std::fmt;
use std::io::{self, BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

/// How long a server gets to answer `shutdown`, then to exit after `exit`.
const SHUTDOWN_WAIT: Duration = Duration::from_secs(1);

type WakeFn = Arc<dyn Fn() + Send + Sync>;

/// The frontend's callback for "a message arrived": shared by every reader
/// thread, and settable after the servers started.
#[derive(Clone, Default)]
pub(crate) struct Waker(Arc<Mutex<Option<WakeFn>>>);

impl fmt::Debug for Waker {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Waker")
    }
}

impl Waker {
    pub(crate) fn set(&self, wake: WakeFn) {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner) = Some(wake);
    }

    fn wake(&self) {
        if let Some(wake) = self
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
        {
            wake();
        }
    }
}

/// What the reader thread passes on.
#[derive(Debug)]
pub(crate) enum Incoming {
    /// A message from the server.
    Message(Value),
    /// The server's output ended: it exited or crashed.
    Closed,
}

/// A running server process.
#[derive(Debug)]
pub(crate) struct Process {
    name: String,
    child: Child,
    outgoing: Option<Sender<Vec<u8>>>,
    incoming: Receiver<Incoming>,
}

impl Process {
    /// Starts `config`'s command in `root`.
    pub(crate) fn spawn(
        config: &ServerConfig,
        root: &Path,
        name: &str,
        waker: &Waker,
    ) -> io::Result<Self> {
        let mut child = Command::new(&config.command)
            .args(&config.args)
            .current_dir(root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let missing = || io::Error::other("the server's pipes are missing");
        let stdin = child.stdin.take().ok_or_else(missing)?;
        let stdout = child.stdout.take().ok_or_else(missing)?;
        let stderr = child.stderr.take().ok_or_else(missing)?;
        let (outgoing, to_write) = mpsc::channel();
        let (received, incoming) = mpsc::channel();
        spawn_thread(format!("lsp-{name}-writer"), move || {
            write_all(stdin, &to_write);
        })?;
        let waker = waker.clone();
        spawn_thread(format!("lsp-{name}-reader"), move || {
            read_all(BufReader::new(stdout), &received, &waker);
        })?;
        let log_name = name.to_owned();
        spawn_thread(format!("lsp-{name}-stderr"), move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                tracing::debug!(server = %log_name, "{line}");
            }
        })?;
        Ok(Self {
            name: name.to_owned(),
            child,
            outgoing: Some(outgoing),
            incoming,
        })
    }

    /// Sends a message, traced as a `child_message` span.
    pub(crate) fn send(&self, message: &Value) {
        let method = message["method"].as_str().unwrap_or("response");
        let span = spans::child_message(&self.name, Direction::Outgoing, method);
        let _entered = span.enter();
        tracing::trace!(%message, "to the server");
        if let Some(outgoing) = &self.outgoing {
            let _ = outgoing.send(rpc::frame(message));
        }
    }

    /// The next thing the reader passed on, without waiting.
    pub(crate) fn try_recv(&self) -> Option<Incoming> {
        self.incoming.try_recv().ok()
    }

    /// How the process ended, waiting a little for it after its output
    /// closed; it is killed if it lingers.
    pub(crate) fn reap(mut self) -> String {
        let status = self.wait_until(Instant::now() + Duration::from_millis(500));
        describe(status)
    }

    /// Asks the server to shut down and exit, as LSP's lifecycle has it; a
    /// server that does not comply in time is killed.
    pub(crate) fn shutdown(mut self, request: i64) {
        self.send(&json!({"jsonrpc": "2.0", "id": request, "method": "shutdown"}));
        let deadline = Instant::now() + SHUTDOWN_WAIT;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.incoming.recv_timeout(left) {
                Ok(Incoming::Message(reply)) if reply["id"] == request => break,
                Ok(Incoming::Message(_)) => {}
                Ok(Incoming::Closed)
                | Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => break,
            }
        }
        self.send(&json!({"jsonrpc": "2.0", "method": "exit"}));
        self.outgoing = None;
        let status = self.wait_until(Instant::now() + SHUTDOWN_WAIT);
        tracing::debug!(server = %self.name, status = %describe(status), "stopped");
    }

    /// Waits for the process until `deadline`, then kills it.
    fn wait_until(&mut self, deadline: Instant) -> Option<ExitStatus> {
        loop {
            match self.child.try_wait() {
                Ok(Some(status)) => return Some(status),
                Ok(None) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(5));
                }
                _ => {
                    let _ = self.child.kill();
                    return self.child.wait().ok();
                }
            }
        }
    }
}

fn spawn_thread(name: String, run: impl FnOnce() + Send + 'static) -> io::Result<()> {
    std::thread::Builder::new().name(name).spawn(run).map(drop)
}

fn write_all(mut stdin: ChildStdin, frames: &Receiver<Vec<u8>>) {
    for frame in frames {
        if stdin
            .write_all(&frame)
            .and_then(|()| stdin.flush())
            .is_err()
        {
            break;
        }
    }
}

fn read_all(mut stdout: impl BufRead, received: &Sender<Incoming>, waker: &Waker) {
    while let Some(message) = read_next(&mut stdout) {
        if received.send(Incoming::Message(message)).is_err() {
            return;
        }
        waker.wake();
    }
    let _ = received.send(Incoming::Closed);
    waker.wake();
}

/// The next message; `None` once the output ended or broke. A message that
/// is not JSON is logged and skipped.
fn read_next(stdout: &mut impl BufRead) -> Option<Value> {
    loop {
        match rpc::read(stdout) {
            Ok(message) => return message,
            Err(err) => {
                let skipped = err.kind() == io::ErrorKind::InvalidData;
                tracing::warn!(%err, skipped, "cannot read a message from the server");
                if !skipped {
                    return None;
                }
            }
        }
    }
}

/// How a process ended, for the status bar.
fn describe(status: Option<ExitStatus>) -> String {
    use std::os::unix::process::ExitStatusExt as _;
    match status {
        Some(status) => match (status.code(), status.signal()) {
            (Some(code), _) => format!("exited with code {code}"),
            (None, Some(signal)) => format!("was killed by signal {signal}"),
            (None, None) => "exited".to_owned(),
        },
        None => "ended".to_owned(),
    }
}
