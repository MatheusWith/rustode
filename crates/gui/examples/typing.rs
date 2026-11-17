//! Types into the editor from inside the process and prints, for each typed
//! frame, the time from the frame's start to the return from its present.
//!
//! Usage: `cargo run --release -p rustode-gui --example typing -- <file> [count]`
//! Run it on the virtual display only (`AGENTS.md` §3).
//!
//! Nothing is injected into the system: every 100 ms one character (or Enter,
//! every 40th time) is added to the frame's own input through
//! `raw_input_hook`, and stamped there. The interval measured is therefore
//! frame start → present return; a real key also waits in the OS and in the
//! event loop before its frame starts, which this leaves out. The `rustode-gui`
//! binary times real keys from their dispatch instead.

use eframe::egui::{self, Event, Key, Modifiers, RawInput, ViewportCommand};
use rustode_gui::EditorApp;
use rustode_gui::latency::PresentLatency;
use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

const INTERVAL: Duration = Duration::from_millis(100);
const TEXT: &str = "let answer = 42; ";

struct Typing {
    editor: EditorApp,
    latency: Rc<RefCell<PresentLatency>>,
    typed: usize,
    count: usize,
    next: Instant,
}

impl Typing {
    fn next_events(&self) -> Vec<Event> {
        if self.typed % 40 == 39 {
            return [true, false]
                .map(|pressed| Event::Key {
                    key: Key::Enter,
                    physical_key: None,
                    pressed,
                    repeat: false,
                    modifiers: Modifiers::NONE,
                })
                .to_vec();
        }
        let ch = TEXT.chars().cycle().nth(self.typed).unwrap_or('x');
        vec![Event::Text(ch.to_string())]
    }
}

impl eframe::App for Typing {
    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw_input: &mut RawInput) {
        let now = Instant::now();
        if self.typed < self.count && now >= self.next {
            raw_input.events.extend(self.next_events());
            self.latency.borrow_mut().key(now);
            self.typed += 1;
            self.next = now + INTERVAL;
        }
    }

    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.typed < self.count {
            ctx.request_repaint_after(self.next.saturating_duration_since(Instant::now()));
        } else {
            ctx.send_viewport_cmd(ViewportCommand::Close);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        self.editor.ui(ui, frame);
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let path = args.next().ok_or("usage: typing <file> [count]")?;
    let count = args.next().map_or(Ok(300), |count| count.parse())?;
    let latency = Rc::new(RefCell::new(PresentLatency::new()));
    let mut editor = EditorApp::open(std::path::Path::new(&path))?;
    editor.time_frames(Rc::clone(&latency));
    let title = editor.window_title();
    let typing = Typing {
        editor,
        latency: Rc::clone(&latency),
        typed: 0,
        count,
        next: Instant::now() + Duration::from_secs(2),
    };
    rustode_gui::run_timed(typing, &title, &latency)?;
    match latency.borrow().report() {
        Some(report) => println!("frame start to present, {count} typed frames: {report}"),
        None => println!("no typed frame was presented"),
    }
    Ok(())
}
