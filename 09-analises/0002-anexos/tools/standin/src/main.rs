use eframe::egui;
// Throwaway stand-in for the future rustode GUI: an eframe/egui window with one TextEdit.
// Prints every key event egui receives (with wall-clock ms) and the text after each change.
use std::time::{SystemTime, UNIX_EPOCH};
fn now_ms() -> f64 { SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs_f64() * 1000.0 }
struct App { text: String, focused: bool }
impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _f: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        ctx.input(|i| for e in &i.events {
            match e {
                egui::Event::Key { key, pressed: true, .. } => println!("RECV {:.3} key={key:?}", now_ms()),
                egui::Event::Text(t) => println!("TEXT {:.3} {t:?}", now_ms()),
                _ => {}
            }
        });
        egui::CentralPanel::default().show_inside(ui, |ui| {
            let r = ui.add(egui::TextEdit::multiline(&mut self.text).desired_width(f32::INFINITY));
            if !self.focused { r.request_focus(); self.focused = true; }
            if r.changed() { println!("BUFFER {:?}", self.text); }
        });
    }
}
fn main() -> eframe::Result<()> {
    eprintln!("standin on WAYLAND_DISPLAY={:?}", std::env::var("WAYLAND_DISPLAY").ok());
    eframe::run_native("rustode-standin", eframe::NativeOptions::default(),
        Box::new(|_| Ok(Box::new(App { text: String::new(), focused: false }))))
}
