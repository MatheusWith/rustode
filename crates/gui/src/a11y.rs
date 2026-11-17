//! Helpers for the AccessKit nodes the window builds itself.

use eframe::egui::{Rect, accesskit};

/// `rect` as AccessKit bounds, in points as egui gives its own nodes.
pub(crate) fn bounds(rect: Rect) -> accesskit::Rect {
    accesskit::Rect {
        x0: rect.min.x.into(),
        y0: rect.min.y.into(),
        x1: rect.max.x.into(),
        y1: rect.max.y.into(),
    }
}
