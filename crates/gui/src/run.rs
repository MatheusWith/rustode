//! Runs the window: eframe on wgpu, on an event loop of our own so that key
//! dispatch and the return from each painted frame can be timed (see
//! [`crate::latency`]). Everything else is eframe's own handler.

use crate::EditorApp;
use crate::latency::PresentLatency;
use eframe::UserEvent;
use rustode_workspace::trace::LatencyReport;
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Instant;
use winit::application::ApplicationHandler;
use winit::event::{DeviceEvent, DeviceId, ElementState, Ime, StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::WindowId;

/// The native window options: wgpu, a 1000×700 window named `rustode`.
#[must_use]
pub fn native_options() -> eframe::NativeOptions {
    eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        viewport: eframe::egui::ViewportBuilder::default()
            .with_app_id("rustode")
            .with_inner_size([1000.0, 700.0]),
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
    run_timed(app, &title, &latency)?;
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
    };
    event_loop.run_app(&mut timed)?;
    Ok(())
}

/// eframe's handler, with key presses and presented frames timed.
struct Timed<'a> {
    eframe: eframe::EframeWinitApplication<'a>,
    latency: Rc<RefCell<PresentLatency>>,
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
            self.latency.borrow_mut().presented(Instant::now());
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
