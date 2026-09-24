//! One repaint: lay the panel out into the bitmap and hand it to Windows.

use super::canvas::Canvas;
use super::layout::Span;
use super::*;

impl Overlay {
    pub(super) fn paint(&mut self, spans: &[Span]) {
        if self.hwnd == 0 || !self.visible {
            return;
        }
        let Some((width, height)) = self.client_size() else {
            return;
        };

        // The bitmap and its fonts depend on the panel size and the DPI only.
        let scale = self.scale;
        let key = (width, height, (scale * 1000.0) as i32);
        if self.canvas_key != Some(key) {
            self.canvas = Canvas::create(width, height, scale);
            self.canvas_key = Some(key);
        }
        let Some(canvas) = self.canvas.as_mut() else {
            return;
        };

        let alpha = (self.config.opacity.clamp(0.0, 1.0) * 255.0) as u8;
        let radius = (10.0 * scale).round() as i32;
        canvas.fill_panel(alpha, radius);
        for span in spans {
            canvas.draw(span);
        }
        canvas.make_text_opaque();
        canvas.present(self.hwnd);
    }

    /// Client area in physical pixels, or `None` if the window has none yet.
    fn client_size(&self) -> Option<(i32, i32)> {
        let mut client = Rect::default();
        // SAFETY: a read-only query about our own window.
        if unsafe { GetClientRect(self.hwnd, &mut client) } == 0 {
            return None;
        }
        let width = client.right - client.left;
        let height = client.bottom - client.top;
        (width > 0 && height > 0).then_some((width, height))
    }
}
