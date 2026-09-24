//! Pure layout: where each name and value rectangle goes.

use crate::config::{Layout, Spacing};

use super::metric::Metric;
use super::*;

/// One drawable piece of text.
pub(super) struct Span {
    pub(super) text: String,
    pub(super) rect: Rect,
    pub(super) align: u32,
    pub(super) color: u32,
    pub(super) value: bool,
}

/// Text colours: names stay muted so the values keep the visual lead; a value
/// turns red once it crosses its warning threshold.
const LABEL_COLOR: u32 = 0x00C0_C0C0;
const VALUE_COLOR: u32 = 0x00FF_FFFF;
const WARN_COLOR: u32 = 0x0050_50FF; // RGB(255, 80, 80)

impl Overlay {
    pub(super) fn rows(&self, snapshot: &Snapshot) -> Vec<(&'static str, String, bool)> {
        let t = self.text();
        Metric::ALL
            .iter()
            .filter(|metric| self.is_visible_metric(**metric))
            .map(|metric| {
                (
                    metric.label(&t),
                    metric.value(snapshot),
                    metric.is_warning(snapshot),
                )
            })
            .collect()
    }

    // ------------------------------------------------------------- window

    /// Vertical distance between rows for the current spacing preset.
    pub(super) fn row_gap(&self) -> f32 {
        match self.settings().spacing {
            Spacing::Tight => 1.0,
            Spacing::Loose => 2.0,
        }
    }

    /// `DrawTextW` flags selecting the configured text alignment.
    pub(super) fn align_dt(&self) -> u32 {
        match self.settings().align {
            Align::Left => 0,
            Align::Center => DT_CENTER,
            Align::Right => DT_RIGHT,
        }
    }

    /// Rendered name↔value gap in whole physical pixels: 2pt rounded **up**, so
    /// it is never smaller than 2pt on any DPI.
    pub(super) fn name_gap(&self) -> i32 {
        (NAME_GAP_PT * PT * self.scale).ceil() as i32
    }

    /// Physical size of the panel that exactly fits the given content widths.
    /// All geometry is in whole pixels so the spans and the panel agree.
    pub(super) fn content_size(&self, rows: usize, label_w: f32, value_w: f32) -> (i32, i32) {
        let s = self.scale;
        let m = (MARGIN * s).round() as i32;
        let gap = self.name_gap();
        let col_gap = (COLUMN_GAP * s).round() as i32;
        let row_h = (ROW_H * s).round() as i32;
        let row_gap = (self.row_gap() * s).round() as i32;
        let label_w = label_w.round() as i32;
        let value_w = value_w.round() as i32;
        let count = rows.max(1) as i32;
        match self.settings().layout {
            Layout::Vertical => (
                m * 2 + label_w + gap + value_w,
                m * 2 + count * row_h + (count - 1) * row_gap,
            ),
            Layout::Grid => {
                let lines = (count + 1) / 2;
                (
                    m * 2 + 2 * (label_w + gap + value_w),
                    m * 2 + lines * row_h + (lines - 1) * row_gap,
                )
            }
            Layout::Horizontal => {
                let col = label_w.max(value_w);
                (
                    m * 2 + count * col + (count - 1) * col_gap,
                    m * 2 + 2 * row_h + gap,
                )
            }
        }
    }

    pub(super) fn build_spans(
        &self,
        rows: &[(&'static str, String, bool)],
        label_w: f32,
        value_w: f32,
    ) -> Vec<Span> {
        let align = self.align_dt();
        let s = self.scale;
        let m = (MARGIN * s).round() as i32;
        let gap = self.name_gap();
        let col_gap = (COLUMN_GAP * s).round() as i32;
        let row_h = (ROW_H * s).round() as i32;
        let row_gap = (self.row_gap() * s).round() as i32;
        let label_w = label_w.round() as i32;
        let value_w = value_w.round() as i32;

        let mut spans = Vec::new();
        let mut cell = |x: i32,
                        y: i32,
                        label: &str,
                        value: &str,
                        value_color: u32,
                        label_align: u32,
                        value_align: u32| {
            let label_right = x + label_w;
            let value_left = label_right + gap;
            spans.push(Span {
                text: label.to_owned(),
                rect: Rect {
                    left: x,
                    top: y,
                    right: label_right,
                    bottom: y + row_h,
                },
                align: label_align,
                color: LABEL_COLOR,
                value: false,
            });
            spans.push(Span {
                text: value.to_owned(),
                rect: Rect {
                    left: value_left,
                    top: y,
                    right: value_left + value_w,
                    bottom: y + row_h,
                },
                align: value_align,
                color: value_color,
                value: true,
            });
        };

        match self.settings().layout {
            Layout::Vertical => {
                // Name flush left, 2pt gap, value immediately after.
                for (index, (label, value, warn)) in rows.iter().enumerate() {
                    let y = m + index as i32 * (row_h + row_gap);
                    let color = if *warn { WARN_COLOR } else { VALUE_COLOR };
                    cell(m, y, label, value, color, align, align);
                }
            }
            Layout::Grid => {
                let cell_w = label_w + gap + value_w;
                for (index, (label, value, warn)) in rows.iter().enumerate() {
                    let column = (index % 2) as i32;
                    let line = (index / 2) as i32;
                    let y = m + line * (row_h + row_gap);
                    let x = m + column * cell_w;
                    let color = if *warn { WARN_COLOR } else { VALUE_COLOR };
                    cell(x, y, label, value, color, align, align);
                }
            }
            Layout::Horizontal => {
                let col = label_w.max(value_w);
                for (index, (label, value, warn)) in rows.iter().enumerate() {
                    let x = m + index as i32 * (col + col_gap);
                    spans.push(Span {
                        text: (*label).to_owned(),
                        rect: Rect {
                            left: x,
                            top: m,
                            right: x + col,
                            bottom: m + row_h,
                        },
                        align,
                        color: LABEL_COLOR,
                        value: false,
                    });
                    spans.push(Span {
                        text: value.to_owned(),
                        rect: Rect {
                            left: x,
                            top: m + row_h + gap,
                            right: x + col,
                            bottom: m + 2 * row_h + gap,
                        },
                        align,
                        color: if *warn { WARN_COLOR } else { VALUE_COLOR },
                        value: true,
                    });
                }
            }
        }
        spans
    }
}
