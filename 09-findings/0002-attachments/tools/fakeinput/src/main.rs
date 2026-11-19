// Throwaway spike tool: types an ASCII string through org_kde_kwin_fake_input.
// Refuses to run unless WAYLAND_DISPLAY names a kwin_wayland --virtual socket (qa-virt-*).
use std::time::{SystemTime, UNIX_EPOCH};
use wayland_client::{protocol::wl_registry, Connection, Dispatch, QueueHandle};
use wayland_protocols_plasma::fake_input::client::org_kde_kwin_fake_input::OrgKdeKwinFakeInput;

struct S { fi: Option<OrgKdeKwinFakeInput> }
impl Dispatch<wl_registry::WlRegistry, ()> for S {
    fn event(s: &mut Self, r: &wl_registry::WlRegistry, e: wl_registry::Event, _: &(), _: &Connection, qh: &QueueHandle<Self>) {
        if let wl_registry::Event::Global { name, interface, version } = e {
            if interface == "org_kde_kwin_fake_input" { s.fi = Some(r.bind(name, version.min(6), qh, ())); }
        }
    }
}
impl Dispatch<OrgKdeKwinFakeInput, ()> for S {
    fn event(_: &mut Self, _: &OrgKdeKwinFakeInput, _: <OrgKdeKwinFakeInput as wayland_client::Proxy>::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}
fn now_ms() -> f64 { SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs_f64() * 1000.0 }
// evdev keycodes (linux/input-event-codes.h) for a-z, 0-9, space, enter
fn code(c: char) -> Option<(u32, bool)> {
    const L: &str = "qwertyuiop"; const M: &str = "asdfghjkl"; const N: &str = "zxcvbnm";
    let lc = c.to_ascii_lowercase(); let up = c.is_ascii_uppercase();
    if let Some(i) = L.find(lc) { return Some((16 + i as u32, up)); }
    if let Some(i) = M.find(lc) { return Some((30 + i as u32, up)); }
    if let Some(i) = N.find(lc) { return Some((44 + i as u32, up)); }
    match c { '1'..='9' => Some((2 + (c as u32 - '1' as u32), false)), '0' => Some((11, false)), ' ' => Some((57, false)), '\n' => Some((28, false)), _ => None }
}
fn main() {
    let wd = std::env::var("WAYLAND_DISPLAY").unwrap_or_default();
    if !wd.starts_with("qa-virt-") { eprintln!("refusing: WAYLAND_DISPLAY={wd:?} is not a qa-virt-* socket"); std::process::exit(2); }
    let text = std::env::args().nth(1).expect("usage: fakeinput <text> [delay_ms]");
    let delay: u64 = std::env::args().nth(2).map(|s| s.parse().unwrap()).unwrap_or(30);
    let conn = Connection::connect_to_env().expect("connect");
    let mut q = conn.new_event_queue(); let qh = q.handle();
    conn.display().get_registry(&qh, ());
    let mut s = S { fi: None }; q.roundtrip(&mut s).unwrap();
    let fi = match s.fi.clone() { Some(f) => f, None => { eprintln!("org_kde_kwin_fake_input not advertised"); std::process::exit(3) } };
    eprintln!("bound org_kde_kwin_fake_input v{} on {wd}", wayland_client::Proxy::version(&fi));
    fi.authenticate("rustode-spike".into(), "typing-latency spike".into());
    q.roundtrip(&mut s).unwrap();
    for c in text.chars() {
        let Some((k, shift)) = code(c) else { eprintln!("skip {c:?}"); continue };
        if shift { fi.keyboard_key(42, 1); }
        println!("SEND {:.3} {c:?} code={k}", now_ms());
        fi.keyboard_key(k, 1); fi.keyboard_key(k, 0);
        if shift { fi.keyboard_key(42, 0); }
        q.roundtrip(&mut s).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(delay));
    }
    eprintln!("done");
}
