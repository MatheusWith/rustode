//! Runs the window: eframe on wgpu, on an event loop of our own so that key
//! dispatch and the return from each painted frame can be timed (see
//! [`crate::latency`]). Everything else is eframe's own handler.

use crate::EditorApp;
use crate::latency::PresentLatency;
use eframe::UserEvent;
use rustode_workspace::bench::Driver;
use rustode_workspace::trace::{FirstFrame, LatencyReport};
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Instant;
use winit::application::ApplicationHandler;
use winit::event::{DeviceEvent, DeviceId, ElementState, Ime, StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::WindowId;

/// The native window options: wgpu, a 1000×700 window named `rustode`,
/// no smaller than VS Code's 400×270 (see [`crate::layout`]). Sizes are in
/// points: eframe applies the platform's scale factor.
#[must_use]
pub fn native_options() -> eframe::NativeOptions {
    eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        viewport: eframe::egui::ViewportBuilder::default()
            .with_app_id("rustode")
            .with_inner_size([1000.0, 700.0])
            .with_min_inner_size(crate::layout::WINDOW_MIN_SIZE),
        ..eframe::NativeOptions::default()
    }
}

/// Opens the window and runs it until it is closed. Returns the
/// input-to-present latency of the keys typed, if any.
///
/// # Errors
/// When the event loop or the window cannot be created.
pub fn run(mut app: EditorApp) -> eframe::Result<Option<LatencyReport>> {
    let latency = Rc::new(RefCell::new(PresentLatency::new()));
    app.time_frames(Rc::clone(&latency));
    let title = app.window_title();
    let bench = app.bench_driver();
    run_with(app, &title, &latency, bench)?;
    let report = latency.borrow().report();
    Ok(report)
}

/// Runs `app` in a window titled `title` until it is closed, on an event
/// loop that gives `latency` each key press and the return from each
/// painted frame. The app reports its own frames to `latency` (see
/// [`EditorApp::time_frames`]).
///
/// # Errors
/// When the event loop or the window cannot be created.
pub fn run_timed(
    app: impl eframe::App + 'static,
    title: &str,
    latency: &Rc<RefCell<PresentLatency>>,
) -> eframe::Result {
    run_with(app, title, latency, None)
}

/// [`run_timed`], also telling the benchmark driver (if any) when each frame
/// was presented.
///
/// # Errors
/// When the event loop or the window cannot be created, or when the loop
/// ends before the window presented a frame (no usable graphics adapter,
/// which eframe reports as a clean exit).
fn run_with(
    app: impl eframe::App + 'static,
    title: &str,
    latency: &Rc<RefCell<PresentLatency>>,
    bench: Option<Rc<RefCell<Driver>>>,
) -> eframe::Result {
    let event_loop = EventLoop::<UserEvent>::with_user_event().build()?;
    let mut options = native_options();
    options.viewport = options.viewport.with_title(title);
    let eframe = eframe::create_native(
        "rustode",
        options,
        Box::new(move |cc| {
            crate::fonts::install(&cc.egui_ctx);
            Ok(Box::new(app))
        }),
        &event_loop,
    );
    let mut timed = Timed {
        eframe,
        latency: Rc::clone(latency),
        first_frame: FirstFrame::requested().then(FirstFrame::since_process_start),
        bench,
        presented: false,
    };
    event_loop.run_app(&mut timed)?;
    ended(timed.presented)
}

/// How a window's event loop ended: eframe turns "no usable graphics adapter"
/// into a clean exit, which would be a silent success (gaps.md G24), so a loop
/// that ends before any frame was presented is an error the user can act on.
fn ended(presented: bool) -> eframe::Result {
    if presented {
        Ok(())
    } else {
        Err(eframe::Error::AppCreation(
            "the window closed before it presented a frame: no usable graphics adapter \
             (run with RUSTODE_LOG=info for the graphics error)"
                .into(),
        ))
    }
}

/// eframe's handler, with key presses and presented frames timed.
struct Timed<'a> {
    eframe: eframe::EframeWinitApplication<'a>,
    latency: Rc<RefCell<PresentLatency>>,
    /// Set when `RUSTODE_REPORT_FIRST_FRAME` is: prints the first presented
    /// frame's time once, for `rustode bench` (startup scenarios).
    first_frame: Option<FirstFrame>,
    /// The benchmark script's driver, told of each presented frame.
    bench: Option<Rc<RefCell<Driver>>>,
    /// Whether any frame was presented.
    presented: bool,
}

impl ApplicationHandler<UserEvent> for Timed<'_> {
    fn new_events(&mut self, event_loop: &ActiveEventLoop, cause: StartCause) {
        self.eframe.new_events(event_loop, cause);
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        self.eframe.resumed(event_loop);
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent) {
        self.eframe.user_event(event_loop, event);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        let redraw = matches!(event, WindowEvent::RedrawRequested);
        if is_typing(&event) {
            self.latency.borrow_mut().key(Instant::now());
        }
        self.eframe.window_event(event_loop, id, event);
        if redraw {
            let at = Instant::now();
            self.latency.borrow_mut().presented(at);
            self.presented = true;
            if let Some(bench) = &self.bench {
                bench.borrow_mut().after_present(at);
            }
            if let Some(elapsed) = self.first_frame.as_mut().and_then(|f| f.presented(at)) {
                eprintln!("{}", FirstFrame::line(elapsed));
            }
        }
    }

    fn device_event(&mut self, event_loop: &ActiveEventLoop, id: DeviceId, event: DeviceEvent) {
        self.eframe.device_event(event_loop, id, event);
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        self.eframe.about_to_wait(event_loop);
    }

    fn suspended(&mut self, event_loop: &ActiveEventLoop) {
        self.eframe.suspended(event_loop);
    }

    fn exiting(&mut self, event_loop: &ActiveEventLoop) {
        self.eframe.exiting(event_loop);
    }

    fn memory_warning(&mut self, event_loop: &ActiveEventLoop) {
        self.eframe.memory_warning(event_loop);
    }
}

/// A key press or an IME commit: input that can change the editor.
fn is_typing(event: &WindowEvent) -> bool {
    match event {
        WindowEvent::KeyboardInput { event, .. } => event.state == ElementState::Pressed,
        WindowEvent::Ime(Ime::Commit(_)) => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::ended;

    #[test]
    fn a_loop_that_ended_without_a_frame_is_an_error_with_a_hint() {
        assert!(ended(true).is_ok());
        let error = ended(false).expect_err("no frame, no success");
        let text = error.to_string();
        assert!(text.contains("no usable graphics adapter"), "{text}");
        assert!(text.contains("RUSTODE_LOG=info"), "{text}");
    }
}
