//! Cached GDI resources: the panel bitmap, its fonts, and text measurement.

use std::ffi::c_void;

use crate::wide::wide;

use super::layout::Span;
use super::*;

/// Cached GDI resources for a fixed bitmap size and font scale.
pub(super) struct Canvas {
    memory_dc: isize,
    bitmap: isize,
    old_bitmap: isize,
    label_font: isize,
    value_font: isize,
    bits: *mut c_void,
    width: i32,
    height: i32,
    /// Reused UTF-16 scratch buffer for `DrawTextW`.
    text: Vec<u16>,
}

impl Canvas {
    /// Panel background: translucent black with rounded corners, fully
    /// transparent everywhere else.
    pub(super) fn fill_panel(&mut self, alpha: u8, radius: i32) {
        let (width, height) = (self.width, self.height);
        let pixels = self.pixels_mut();
        pixels.fill(0);
        for (y, row) in pixels.chunks_exact_mut(width as usize * 4).enumerate() {
            for (x, pixel) in row.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                if inside_rounded(x as i32, y as i32, width, height, radius) {
                    pixel[3] = alpha;
                }
            }
        }
    }

    /// Draws one span of text into the bitmap.
    pub(super) fn draw(&mut self, span: &Span) {
        let font = if span.value {
            self.value_font
        } else {
            self.label_font
        };
        if font == 0 {
            return;
        }
        self.text.clear();
        self.text.extend(span.text.encode_utf16());
        let mut rect = span.rect;
        // SAFETY: the DC and font are live; `text` outlives the call.
        unsafe {
            SelectObject(self.memory_dc, font);
            SetTextColor(self.memory_dc, span.color);
            DrawTextW(
                self.memory_dc,
                self.text.as_mut_ptr(),
                self.text.len() as i32,
                &mut rect,
                DT_SINGLELINE | DT_VCENTER | DT_NOPREFIX | span.align,
            );
        }
    }

    /// The text must stay fully opaque over the translucent panel, so every
    /// pixel the text touched gets full alpha.
    pub(super) fn make_text_opaque(&mut self) {
        for pixel in self.pixels_mut().as_chunks_mut::<4>().0 {
            if pixel[0] != 0 || pixel[1] != 0 || pixel[2] != 0 {
                pixel[3] = 255;
            }
        }
    }

    /// Hands the finished bitmap to the compositor.
    pub(super) fn present(&self, hwnd: isize) {
        let mut window = Rect::default();
        // SAFETY: queries and updates our own window.
        unsafe {
            GetWindowRect(hwnd, &mut window);
            let position = Point {
                x: window.left,
                y: window.top,
            };
            let size = Point {
                x: self.width,
                y: self.height,
            };
            let src = Point { x: 0, y: 0 };
            let blend = BlendFunction {
                blend_op: 0,
                blend_flags: 0,
                source_constant_alpha: 255,
                alpha_format: AC_SRC_ALPHA,
            };
            let updated = UpdateLayeredWindow(
                hwnd,
                0,
                &position,
                &size,
                self.memory_dc,
                &src,
                0,
                &blend,
                ULW_ALPHA,
            );
            if updated == 0 {
                let error = GetLastError();
                crate::diag::log(&format!("UpdateLayeredWindow failed: {error}"));
            }
        }
    }

    /// The bitmap as BGRA bytes, top-down.
    fn pixels_mut(&mut self) -> &mut [u8] {
        let len = self.width as usize * self.height as usize * 4;
        // SAFETY: the DIB section owns exactly `width * height * 4` bytes.
        unsafe { std::slice::from_raw_parts_mut(self.bits as *mut u8, len) }
    }

    pub(super) fn create(width: i32, height: i32, scale: f32) -> Option<Self> {
        // SAFETY: all handles are checked and released in `Drop`.
        unsafe {
            let screen = CreateCompatibleDC(0);
            if screen == 0 {
                return None;
            }
            let memory_dc = CreateCompatibleDC(screen);
            DeleteDC(screen);
            if memory_dc == 0 {
                return None;
            }
            let info = BitmapInfo {
                header: BitmapInfoHeader {
                    size: std::mem::size_of::<BitmapInfoHeader>() as u32,
                    width,
                    height: -height,
                    planes: 1,
                    bit_count: 32,
                    compression: BI_RGB,
                    size_image: (width * height * 4) as u32,
                    x_pels_per_meter: 0,
                    y_pels_per_meter: 0,
                    clr_used: 0,
                    clr_important: 0,
                },
                colors: [RgbQuad {
                    blue: 0,
                    green: 0,
                    red: 0,
                    reserved: 0,
                }],
            };
            let mut bits: *mut c_void = std::ptr::null_mut();
            let bitmap = CreateDIBSection(memory_dc, &info, DIB_RGB_COLORS, &mut bits, 0, 0);
            if bitmap == 0 || bits.is_null() {
                DeleteDC(memory_dc);
                return None;
            }
            let old_bitmap = SelectObject(memory_dc, bitmap);
            SetBkMode(memory_dc, TRANSPARENT);

            let face = wide("Microsoft YaHei");
            let label_font = CreateFontW(
                -(LABEL_SIZE * scale).round() as i32,
                0,
                0,
                0,
                400,
                0,
                0,
                0,
                1,
                0,
                0,
                5,
                0,
                face.as_ptr(),
            );
            let value_font = CreateFontW(
                -(VALUE_SIZE * scale).round() as i32,
                0,
                0,
                0,
                600,
                0,
                0,
                0,
                1,
                0,
                0,
                5,
                0,
                face.as_ptr(),
            );

            Some(Self {
                memory_dc,
                bitmap,
                old_bitmap,
                label_font,
                value_font,
                bits,
                width,
                height,
                text: Vec::new(),
            })
        }
    }
}

impl Drop for Canvas {
    fn drop(&mut self) {
        // SAFETY: objects were created together and are released once.
        unsafe {
            SelectObject(self.memory_dc, self.old_bitmap);
            DeleteObject(self.bitmap);
            if self.label_font != 0 {
                DeleteObject(self.label_font);
            }
            if self.value_font != 0 {
                DeleteObject(self.value_font);
            }
            DeleteDC(self.memory_dc);
        }
    }
}

/// A throwaway GDI DC with the two text fonts selected, used only to measure
/// how wide a label or value will render so the panel can hug its content.
pub(super) struct TextMetrics {
    dc: isize,
    label_font: isize,
    value_font: isize,
    old_font: isize,
}

impl TextMetrics {
    pub(super) fn create(scale: f32) -> Option<Self> {
        // SAFETY: handles are checked and released in `Drop`.
        unsafe {
            let dc = CreateCompatibleDC(0);
            if dc == 0 {
                return None;
            }
            let face = wide("Microsoft YaHei");
            let label_font = CreateFontW(
                -(LABEL_SIZE * scale).round() as i32,
                0,
                0,
                0,
                400,
                0,
                0,
                0,
                1,
                0,
                0,
                5,
                0,
                face.as_ptr(),
            );
            let value_font = CreateFontW(
                -(VALUE_SIZE * scale).round() as i32,
                0,
                0,
                0,
                600,
                0,
                0,
                0,
                1,
                0,
                0,
                5,
                0,
                face.as_ptr(),
            );
            let old_font = SelectObject(dc, label_font);
            Some(Self {
                dc,
                label_font,
                value_font,
                old_font,
            })
        }
    }

    /// Width in physical pixels of `text` in the label or value font.
    pub(super) fn width(&self, text: &str, value: bool) -> f32 {
        // SAFETY: `self.dc` is live; the font handles belong to this DC.
        unsafe {
            SelectObject(
                self.dc,
                if value {
                    self.value_font
                } else {
                    self.label_font
                },
            );
            let mut units: Vec<u16> = text.encode_utf16().collect();
            let mut size = Size::default();
            GetTextExtentPoint32W(self.dc, units.as_mut_ptr(), units.len() as i32, &mut size);
            size.cx as f32
        }
    }
}

impl Drop for TextMetrics {
    fn drop(&mut self) {
        // SAFETY: objects were created together and are released once.
        unsafe {
            SelectObject(self.dc, self.old_font);
            if self.label_font != 0 {
                DeleteObject(self.label_font);
            }
            if self.value_font != 0 {
                DeleteObject(self.value_font);
            }
            DeleteDC(self.dc);
        }
    }
}

/// True if the pixel lies inside a rounded rectangle of the given radius.
pub(super) fn inside_rounded(x: i32, y: i32, width: i32, height: i32, radius: i32) -> bool {
    let radius = radius.min(width / 2).min(height / 2).max(0);
    if radius == 0 {
        return true;
    }
    let cx = if x < radius {
        radius
    } else if x >= width - radius {
        width - radius - 1
    } else {
        return true;
    };
    let cy = if y < radius {
        radius
    } else if y >= height - radius {
        height - radius - 1
    } else {
        return true;
    };
    let dx = x - cx;
    let dy = y - cy;
    dx * dx + dy * dy <= radius * radius
}
