//! Linux (WebKitGTK): Tauri packs child webviews into the window's vertical
//! box, which ignores positions. ReMa moves both webviews into a
//! `gtk::Overlay` instead: the app fills the window and the browser is
//! placed on top of it with margins, exactly where the frontend reserved
//! space. macOS and Windows position child webviews natively.

use gtk::prelude::*;
use tauri::Webview;

use crate::{error::AppResult, models::browser::BrowserBounds};

/// Positions the page with margins on all four sides, so its size never
/// depends on the page's own preferred size (which would stop it shrinking).
fn place(widget: &gtk::Widget, bounds: BrowserBounds) {
    let (width, height) = widget
        .parent()
        .map(|p| (p.allocated_width(), p.allocated_height()))
        .unwrap_or_default();
    let x = bounds.x.max(0.0).round() as i32;
    let y = bounds.y.max(0.0).round() as i32;
    let w = bounds.width.max(1.0).round() as i32;
    let h = bounds.height.max(1.0).round() as i32;
    widget.set_halign(gtk::Align::Fill);
    widget.set_valign(gtk::Align::Fill);
    widget.set_size_request(1, 1);
    widget.set_margin_start(x);
    widget.set_margin_top(y);
    widget.set_margin_end((width - x - w).max(0));
    widget.set_margin_bottom((height - y - h).max(0));
    widget.queue_resize();
}

/// Moves a newly created browser webview into the overlay.
pub fn embed(view: &Webview, bounds: BrowserBounds) -> AppResult<()> {
    view.with_webview(move |platform| {
        let widget: gtk::Widget = platform.inner().upcast();
        let Some(vbox) = widget.parent().and_then(|p| p.downcast::<gtk::Box>().ok()) else {
            return;
        };
        let overlay = vbox
            .children()
            .into_iter()
            .find_map(|c| c.downcast::<gtk::Overlay>().ok())
            .unwrap_or_else(|| {
                // First time: move ReMa's own webview into a new overlay.
                let overlay = gtk::Overlay::new();
                if let Some(app_view) = vbox.children().into_iter().find(|c| *c != widget) {
                    vbox.remove(&app_view);
                    overlay.add(&app_view);
                }
                vbox.pack_start(&overlay, true, true, 0);
                overlay.show_all();
                overlay
            });
        vbox.remove(&widget);
        overlay.add_overlay(&widget);
        place(&widget, bounds);
        widget.show();
    })
    .map_err(|e| crate::error::AppError::internal(e.to_string()))
}

pub fn set_bounds(view: &Webview, bounds: BrowserBounds) -> AppResult<()> {
    view.with_webview(move |platform| {
        let widget: gtk::Widget = platform.inner().upcast();
        place(&widget, bounds);
    })
    .map_err(|e| crate::error::AppError::internal(e.to_string()))
}
