use super::{
    Metric, Preferences,
    sampling::{self, Device, Frame, Snapshot},
};
use crate::graphics::{DrawPass, canvas_result};
use std::sync::Arc;
use windows_canvas::{
    Brush, ColorF, Ellipse, FontWeight, ParagraphAlignment, Rect, RoundedRect, TextAlignment,
    TextFormat, Vector2, WordWrapping,
};

struct CompactBrushes<'a> {
    text: &'a Brush,
    secondary: &'a Brush,
    accent: &'a Brush,
    track: &'a Brush,
}

pub(crate) struct Drawing {
    family: String,
    label: TextFormat,
    value: TextFormat,
    rate: TextFormat,
    disk_value: TextFormat,
    axis: TextFormat,
    compact_value: TextFormat,
    compact_rate: TextFormat,
    compact_label: TextFormat,
    compact_center: TextFormat,
    compact_unit: TextFormat,
    detail: TextFormat,
    small: TextFormat,
    small_right: TextFormat,
    snapshot: Arc<Snapshot>,
    was_paused: bool,
}
impl Drawing {
    pub(crate) fn new() -> windows::core::Result<Self> {
        let family = crate::font();
        let make = |size, weight| {
            TextFormat::with_weight(&family, size, FontWeight(weight)).map(|f| {
                f.with_paragraph_alignment(ParagraphAlignment::Center)
                    .with_word_wrapping(WordWrapping::NoWrap)
            })
        };
        let label = canvas_result(make(12.0, 600))?;
        let detail = canvas_result(make(11.0, 400))?;
        let small = canvas_result(make(10.0, 400))?;
        let small_right = canvas_result(make(10.0, 400))?.with_alignment(TextAlignment::Trailing);
        let value = canvas_result(make(18.0, 500))?.with_alignment(TextAlignment::Trailing);
        let rate = canvas_result(make(17.0, 400))?.with_alignment(TextAlignment::Trailing);
        let disk_value = canvas_result(make(17.0, 500))?;
        let axis = canvas_result(make(9.0, 400))?.with_alignment(TextAlignment::Trailing);
        let compact_value = canvas_result(make(15.0, 500))?.with_alignment(TextAlignment::Center);
        let compact_rate = canvas_result(make(17.0, 500))?;
        let compact_label = canvas_result(make(11.0, 500))?;
        let compact_center = canvas_result(make(11.0, 500))?.with_alignment(TextAlignment::Center);
        let compact_unit = canvas_result(make(9.0, 400))?;
        for format in [
            &label,
            &detail,
            &small,
            &small_right,
            &value,
            &rate,
            &disk_value,
            &axis,
            &compact_value,
            &compact_rate,
            &compact_label,
            &compact_center,
            &compact_unit,
        ] {
            crate::graphics::ellipsis(format)?;
        }
        Ok(Self {
            family,
            label,
            value,
            rate,
            disk_value,
            axis,
            compact_value,
            compact_rate,
            compact_label,
            compact_center,
            compact_unit,
            detail,
            small,
            small_right,
            snapshot: sampling::snapshot(),
            was_paused: false,
        })
    }
    pub(crate) fn paint(
        &mut self,
        target: &DrawPass<'_>,
        options: &Preferences,
        bounds: Rect,
        text: &Brush,
        dim: &Brush,
        dark: bool,
        ink: f32,
    ) -> windows::core::Result<()> {
        if self.family != crate::font() {
            let mut replacement = Self::new()?;
            replacement.snapshot = self.snapshot.clone();
            replacement.was_paused = self.was_paused;
            *self = replacement;
        }
        if !options.paused || !self.was_paused {
            self.snapshot = sampling::snapshot();
        }
        self.was_paused = options.paused;
        target.push_clip(&bounds);
        let width = (bounds.right - bounds.left - 8.0).max(1.0);
        let height = bounds.bottom - bounds.top;
        let rows = options.metrics.len().max(1);
        let gap = 7.0;
        let tiles = if options.compact {
            compact_tiles(
                &options.metrics,
                Rect::from_xywh(
                    bounds.left + 4.0,
                    bounds.top,
                    width,
                    (height - 28.0).max(1.0),
                ),
            )
        } else {
            Vec::new()
        };
        let heights = if options.compact {
            Vec::new()
        } else {
            standard_heights(
                &options.metrics,
                (height - 28.0 - gap * (rows - 1) as f32).max(1.0),
            )
        };
        let mut standard_top = bounds.top;
        let card = canvas_result(target.create_solid_brush(ColorF::new(
            ink,
            ink,
            ink,
            if dark { 0.045 } else { 0.028 },
        )))?;
        let edge = canvas_result(target.create_solid_brush(ColorF::new(
            ink,
            ink,
            ink,
            if dark { 0.055 } else { 0.035 },
        )))?;
        let subtle = canvas_result(target.create_solid_brush(ColorF::new(ink, ink, ink, 0.055)))?;
        let secondary = canvas_result(target.create_solid_brush(ColorF::new(
            ink,
            ink,
            ink,
            if dark { 0.60 } else { 0.58 },
        )))?;
        for (index, metric) in options.metrics.iter().enumerate() {
            let tile = if options.compact {
                tiles[index]
            } else {
                let tile = Rect::from_xywh(bounds.left + 4.0, standard_top, width, heights[index]);
                let row_height = heights[index];
                standard_top += row_height + gap;
                tile
            };
            let left = tile.left;
            let top = tile.top;
            let row_height = tile.bottom - tile.top;
            let card_width = tile.right - tile.left;
            let shape = RoundedRect {
                rect: tile,
                radius_x: 8.0,
                radius_y: 8.0,
            };
            target.fill_rounded_rect(&shape, &card);
            target.draw_rounded_rect(&shape, &edge, 0.6);
            target.push_clip(&shape.rect);
            let x = left + 12.0;
            let y = top + 4.0;
            let w = (card_width - 24.0).max(1.0);
            let color = color(*metric, dark);
            let accent = canvas_result(target.create_solid_brush(color))?;
            let device = selected_device(&self.snapshot, *metric, options);
            let (value, detail) = presentation(&self.snapshot, *metric, device);
            let throughput = matches!(metric, Metric::Network | Metric::Disk);
            if options.compact {
                self.paint_compact(
                    target,
                    *metric,
                    device,
                    &value,
                    tile,
                    CompactBrushes {
                        text,
                        secondary: &secondary,
                        accent: &accent,
                        track: &subtle,
                    },
                )?;
                target.pop_clip();
                continue;
            }
            let area = canvas_result(target.create_solid_brush(ColorF::new(
                color.r,
                color.g,
                color.b,
                if dark { 0.11 } else { 0.075 },
            )))?;
            target.fill_ellipse(
                &Ellipse::circle(Vector2::new(x + 3.0, y + 10.0), 2.5),
                &accent,
            );
            if *metric == Metric::Disk {
                self.paint_disk(
                    target,
                    device,
                    options.compact,
                    x,
                    y,
                    w,
                    row_height,
                    text,
                    &secondary,
                    &accent,
                    &area,
                    &subtle,
                )?;
            } else {
                target.clipped_text(
                    metric.label(),
                    &self.label,
                    &Rect::from_xywh(x + 10.0, y, w * 0.34, 23.0),
                    dim,
                );
                target.clipped_text(
                    &value,
                    if throughput { &self.rate } else { &self.value },
                    &Rect::from_xywh(x + w * 0.34, y - 1.0, w * 0.66, 25.0),
                    text,
                );
                if *metric == Metric::Memory {
                    target.clipped_text(
                        &detail,
                        &self.detail,
                        &Rect::from_xywh(x, y + 23.0, w, 17.0),
                        &secondary,
                    );
                    let track = RoundedRect {
                        rect: Rect::from_xywh(x, y + (row_height - 14.0).max(41.0), w, 4.0),
                        radius_x: 2.0,
                        radius_y: 2.0,
                    };
                    target.fill_rounded_rect(&track, &subtle);
                    if let Some((used, total)) = self.snapshot.memory {
                        let occupied = w * (used as f64 / total as f64).clamp(0.0, 1.0) as f32;
                        if occupied > 0.0 {
                            target.push_clip(&Rect::from_xywh(x, track.rect.top, occupied, 4.0));
                            target.fill_rounded_rect(&track, &accent);
                            target.pop_clip();
                        }
                    }
                } else {
                    let caption = *metric != Metric::Cpu || self.snapshot.cpu.is_none();
                    let graph_height = row_height - if caption { 52.0 } else { 38.0 };
                    if graph_height >= 14.0 {
                        self.graph(
                            target,
                            *metric,
                            device.map(|d| d.id.as_str()),
                            Rect::from_xywh(x, y + 28.0, w, graph_height),
                            &accent,
                            &area,
                            &subtle,
                            &secondary,
                        )?;
                    }
                    if caption {
                        target.clipped_text(
                            &detail,
                            &self.small,
                            &Rect::from_xywh(x, y + row_height - 22.0, w, 16.0),
                            &secondary,
                        );
                    }
                }
            }
            target.pop_clip();
        }
        let footer = if self.snapshot.unavailable {
            crate::i18n::text("monitor-unavailable").to_string()
        } else if options.paused {
            crate::i18n::text("monitor-paused").to_string()
        } else {
            format!(
                "{} · {} s",
                crate::i18n::text("monitor-live"),
                options.interval
            )
        };
        let status = canvas_result(target.create_solid_brush(
            if options.paused || self.snapshot.unavailable {
                ColorF::new(0.88, 0.62, 0.24, 1.0)
            } else {
                ColorF::new(0.24, 0.72, 0.48, 1.0)
            },
        ))?;
        let footer_top = bounds.bottom - 22.0;
        target.fill_ellipse(
            &Ellipse::circle(Vector2::new(bounds.left + 12.0, footer_top + 10.0), 2.5),
            &status,
        );
        target.clipped_text(
            &footer,
            &self.small,
            &Rect::from_xywh(bounds.left + 21.0, footer_top, width * 0.46, 20.0),
            &secondary,
        );
        if !options.compact {
            target.clipped_text(
                crate::i18n::text("monitor-history"),
                &self.small_right,
                &Rect::from_xywh(bounds.left + width * 0.50, footer_top, width * 0.50, 20.0),
                &secondary,
            );
        }
        target.pop_clip();
        Ok(())
    }
    fn paint_compact(
        &self,
        target: &DrawPass<'_>,
        metric: Metric,
        device: Option<&Device>,
        value: &str,
        bounds: Rect,
        brushes: CompactBrushes<'_>,
    ) -> windows::core::Result<()> {
        let CompactBrushes {
            text,
            secondary,
            accent,
            track,
        } = brushes;
        let width = bounds.right - bounds.left;
        let height = bounds.bottom - bounds.top;
        if matches!(metric, Metric::Network | Metric::Disk) {
            let short = height < 58.0;
            let header_height = if short { 12.0 } else { 14.0 };
            let number_height = if short { 20.0 } else { 22.0 };
            let caption_height = if short { 11.0 } else { 13.0 };
            let x = bounds.left + 10.0;
            let top = bounds.top + (height - header_height - number_height - caption_height) / 2.0;
            target.fill_ellipse(
                &Ellipse::circle(Vector2::new(x + 2.0, top + 7.0), 2.0),
                accent,
            );
            target.clipped_text(
                metric.label(),
                &self.compact_label,
                &Rect::from_xywh(x + 10.0, top, width - 30.0, header_height),
                secondary,
            );
            let readings = device.and_then(|d| d.rate);
            let cell = (width - 20.0) / 2.0;
            for index in 0..2 {
                let left = x + index as f32 * cell;
                let direction = if metric == Metric::Network {
                    if index == 0 { "↓" } else { "↑" }
                } else {
                    crate::i18n::text(if index == 0 {
                        "monitor-read"
                    } else {
                        "monitor-write"
                    })
                };
                let reading = readings.map(|(a, b)| if index == 0 { a } else { b });
                let (number, unit) =
                    reading.map_or_else(|| ("—".into(), String::new()), rate_parts);
                target.clipped_text(
                    &number,
                    &self.compact_rate,
                    &Rect::from_xywh(left, top + header_height, cell - 3.0, number_height),
                    text,
                );
                target.clipped_text(
                    &format!("{direction} {unit}"),
                    &self.compact_unit,
                    &Rect::from_xywh(
                        left,
                        top + header_height + number_height,
                        cell - 3.0,
                        caption_height,
                    ),
                    secondary,
                );
            }
        } else {
            let horizontal = width >= 110.0 && width > height * 1.6;
            let radius = if horizontal {
                ((height - 12.0) / 2.0).clamp(16.0, 26.0)
            } else {
                ((height - 30.0) / 2.0).clamp(22.0, 28.0)
            };
            let center = Vector2::new(
                if horizontal {
                    bounds.left + radius + 10.0
                } else {
                    (bounds.left + bounds.right) / 2.0
                },
                if horizontal {
                    (bounds.top + bounds.bottom) / 2.0
                } else {
                    bounds.top + (height - 2.0 * radius - 18.0) / 2.0 + radius
                },
            );
            let percent = match metric {
                Metric::Cpu => self.snapshot.cpu,
                Metric::Memory => self
                    .snapshot
                    .memory
                    .map(|(used, total)| used as f64 / total.max(1) as f64 * 100.0),
                _ => device.and_then(|d| d.usage),
            };
            target.draw_ellipse(&Ellipse::circle(center, radius), track, 3.0);
            if let Some(percent) = percent.filter(|p| p.is_finite()) {
                target.progress_arc(
                    center,
                    radius,
                    (percent / 100.0).clamp(0.0, 1.0) as f32,
                    accent,
                    3.0,
                )?;
            }
            target.clipped_text(
                value,
                &self.compact_value,
                &Rect::from_xywh(center.x - radius, center.y - 12.0, radius * 2.0, 24.0),
                text,
            );
            let label = if horizontal {
                Rect::from_xywh(
                    center.x + radius + 9.0,
                    center.y - 9.0,
                    (bounds.right - center.x - radius - 15.0).max(0.0),
                    18.0,
                )
            } else {
                Rect::from_xywh(
                    bounds.left + 4.0,
                    center.y + radius + 5.0,
                    width - 8.0,
                    16.0,
                )
            };
            target.clipped_text(
                metric.label(),
                if horizontal {
                    &self.compact_label
                } else {
                    &self.compact_center
                },
                &label,
                secondary,
            );
        }
        Ok(())
    }
    fn paint_disk(
        &self,
        target: &DrawPass<'_>,
        device: Option<&Device>,
        compact: bool,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        text: &Brush,
        secondary: &Brush,
        accent: &Brush,
        area: &Brush,
        grid: &Brush,
    ) -> windows::core::Result<()> {
        target.clipped_text(
            Metric::Disk.label(),
            &self.label,
            &Rect::from_xywh(x + 10.0, y, width * 0.5, 19.0),
            secondary,
        );
        let name = device.map_or(crate::i18n::text("monitor-unavailable"), |d| {
            d.name.trim_end_matches('\\')
        });
        target.clipped_text(
            name,
            &self.small_right,
            &Rect::from_xywh(x + width * 0.5, y, width * 0.5, 19.0),
            secondary,
        );
        let write_color = write_color();
        let write = canvas_result(target.create_solid_brush(write_color))?;
        let pair = width >= 220.0;
        let readings = device.and_then(|d| d.rate);
        for index in 0..2 {
            let left = if pair {
                x + index as f32 * (width / 2.0 + 3.0)
            } else {
                x
            };
            let top = y + 22.0 + if pair { 0.0 } else { index as f32 * 18.0 };
            let column = if pair { width / 2.0 - 7.0 } else { width };
            let tint = if index == 0 { accent } else { &write };
            target.fill_ellipse(
                &Ellipse::circle(Vector2::new(left + 2.0, top + 11.0), 2.0),
                tint,
            );
            target.clipped_text(
                crate::i18n::text(if index == 0 {
                    "monitor-read"
                } else {
                    "monitor-write"
                }),
                &self.small,
                &Rect::from_xywh(left + 7.0, top, 29.0, 23.0),
                secondary,
            );
            self.rate_number(
                target,
                readings.map(|(a, b)| if index == 0 { a } else { b }),
                Rect::from_xywh(left + 37.0, top - 1.0, (column - 37.0).max(1.0), 25.0),
                text,
                secondary,
            )?;
        }
        if !compact && height >= 92.0 {
            self.graph(
                target,
                Metric::Disk,
                device.map(|d| d.id.as_str()),
                Rect::from_xywh(x, y + 51.0, width, height - 77.0),
                accent,
                area,
                grid,
                secondary,
            )?;
        }
        if pair && height >= 58.0 {
            let capacity = device
                .filter(|d| d.total > 0)
                .and_then(|d| {
                    d.used.map(|used| {
                        format!(
                            "{} {} / {}",
                            crate::i18n::text("monitor-used"),
                            bytes(used as f64),
                            bytes(d.total as f64)
                        )
                    })
                })
                .unwrap_or_else(|| crate::i18n::text("monitor-unavailable").into());
            let caption = if device.is_none() {
                crate::i18n::text("monitor-unavailable").into()
            } else if readings.is_none() {
                format!("{} · {capacity}", crate::i18n::text("monitor-collecting"))
            } else {
                capacity
            };
            target.clipped_text(
                &caption,
                &self.small,
                &Rect::from_xywh(x, y + height - 22.0, width, 16.0),
                secondary,
            );
        }
        Ok(())
    }
    fn rate_number(
        &self,
        target: &DrawPass<'_>,
        value: Option<f64>,
        rect: Rect,
        text: &Brush,
        secondary: &Brush,
    ) -> windows::core::Result<()> {
        let (number, unit) = value.map_or_else(|| ("—".into(), String::new()), rate_parts);
        let layout = canvas_result(windows_canvas::TextLayout::new(
            &number,
            &self.disk_value,
            4096.0,
            25.0,
        ))?;
        let number_width = layout
            .metrics()
            .width_including_trailing_whitespace
            .min(rect.right - rect.left);
        target.clipped_text(
            &number,
            &self.disk_value,
            &Rect::from_xywh(rect.left, rect.top, number_width + 1.0, 25.0),
            text,
        );
        if !unit.is_empty() {
            target.clipped_text(
                &unit,
                &self.small,
                &Rect::from_xywh(
                    rect.left + number_width + 3.0,
                    rect.top + 3.0,
                    (rect.right - rect.left - number_width - 3.0).max(0.0),
                    22.0,
                ),
                secondary,
            );
        }
        Ok(())
    }
    fn graph(
        &self,
        target: &DrawPass<'_>,
        metric: Metric,
        id: Option<&str>,
        rect: Rect,
        accent: &Brush,
        area: &Brush,
        grid: &Brush,
        label: &Brush,
    ) -> windows::core::Result<()> {
        let samples: Vec<_> = self
            .snapshot
            .history
            .iter()
            .map(|f| (f.at, plot_value(f, metric, id, false), f.break_before))
            .collect();
        let writes: Vec<_> = if metric == Metric::Disk {
            self.snapshot
                .history
                .iter()
                .map(|f| (f.at, plot_value(f, metric, id, true), f.break_before))
                .collect()
        } else {
            Vec::new()
        };
        let percent = matches!(metric, Metric::Cpu | Metric::Gpu);
        let maximum = chart_maximum(
            metric,
            samples.iter().chain(&writes).filter_map(|(_, v, _)| *v),
        );
        let margin = if percent { 30.0 } else { 51.0 };
        let plot = Rect::from_xywh(
            rect.left + margin,
            rect.top,
            (rect.right - rect.left - margin).max(1.0),
            rect.bottom - rect.top,
        );
        for fraction in [0.0, 0.5, 1.0] {
            let y = plot.top + (plot.bottom - plot.top) * fraction;
            target.draw_line(
                Vector2::new(plot.left, y),
                Vector2::new(plot.right, y),
                grid,
                0.8,
            );
            let tick = if percent {
                format!("{:.0}%", maximum * f64::from(1.0 - fraction))
            } else if fraction == 1.0 {
                "0".into()
            } else {
                let (number, unit) = rate_parts(maximum * f64::from(1.0 - fraction));
                format!("{number}{unit}")
            };
            let top = if fraction == 0.0 {
                y - 2.0
            } else if fraction == 1.0 {
                y - 12.0
            } else {
                y - 6.0
            };
            target.clipped_text(
                &tick,
                &self.axis,
                &Rect::from_xywh(rect.left, top, margin - 6.0, 14.0),
                label,
            );
        }
        target.draw_line(
            Vector2::new(plot.left, plot.top),
            Vector2::new(plot.left, plot.bottom),
            grid,
            0.8,
        );
        graph_series(
            target,
            &samples,
            self.snapshot.at,
            plot,
            maximum,
            accent,
            area,
        )?;
        if metric == Metric::Disk {
            let c = write_color();
            // Write series uses a distinct blue; both directions share one scale.
            let stroke = canvas_result(target.create_solid_brush(c))?;
            let fill = canvas_result(target.create_solid_brush(ColorF::new(c.r, c.g, c.b, 0.055)))?;
            graph_series(
                target,
                &writes,
                self.snapshot.at,
                plot,
                maximum,
                &stroke,
                &fill,
            )?;
        }
        Ok(())
    }
}

fn standard_heights(metrics: &[Metric], available: f32) -> Vec<f32> {
    if metrics.is_empty() {
        return Vec::new();
    }
    if metrics.len() == 1 {
        return vec![available];
    }
    let memory = metrics.contains(&Metric::Memory);
    let memory_height = (available * 0.12).clamp(48.0, 64.0).min(available);
    let weight = |metric| match metric {
        Metric::Memory => 0.0,
        Metric::Gpu => 1.1,
        Metric::Disk => 1.3,
        _ => 1.0,
    };
    let total: f32 = metrics.iter().map(|m| weight(*m)).sum();
    let rest = (available - if memory { memory_height } else { 0.0 }).max(0.0);
    metrics
        .iter()
        .map(|m| {
            if *m == Metric::Memory {
                memory_height
            } else {
                rest * weight(*m) / total
            }
        })
        .collect()
}
/// Flow in configured order. Percentage tiles occupy two units, paired rates three.
/// Seven units fit a rate tile and two gauges, keeping all five metrics in two rows.
/// Incomplete rows expand to fill the panel instead of leaving empty grid cells.
fn compact_tiles(metrics: &[Metric], bounds: Rect) -> Vec<Rect> {
    let weight = |metric: &Metric| {
        if matches!(metric, Metric::Network | Metric::Disk) {
            3
        } else {
            2
        }
    };
    let mut rows = Vec::new();
    let mut start = 0;
    let mut units = 0;
    for (index, metric) in metrics.iter().enumerate() {
        let next = weight(metric);
        if units + next > 7 {
            rows.push((start, index, units));
            start = index;
            units = 0;
        }
        units += next;
    }
    if units > 0 {
        rows.push((start, metrics.len(), units));
    }
    let gap = 6.0;
    let height = ((bounds.bottom - bounds.top - gap * rows.len().saturating_sub(1) as f32)
        / rows.len().max(1) as f32)
        .max(1.0);
    let mut tiles = Vec::with_capacity(metrics.len());
    for (row, (start, end, units)) in rows.iter().copied().enumerate() {
        let available = (bounds.right - bounds.left - gap * (end - start - 1) as f32).max(1.0);
        let mut left = bounds.left;
        for metric in &metrics[start..end] {
            let width = available * weight(metric) as f32 / units as f32;
            tiles.push(Rect::from_xywh(
                left,
                bounds.top + row as f32 * (height + gap),
                width,
                height,
            ));
            left += width + gap;
        }
    }
    tiles
}

fn rate_parts(value: f64) -> (String, String) {
    let (value, unit) = [(3, "GiB/s"), (2, "MiB/s"), (1, "KiB/s")]
        .into_iter()
        .find_map(|(power, unit)| {
            let scale = 1024_f64.powi(power);
            (value >= scale).then_some((value / scale, unit))
        })
        .unwrap_or((value, "B/s"));
    let number = if value >= 100.0 || unit == "B/s" {
        format!("{value:.0}")
    } else {
        format!("{value:.1}")
    };
    (number, unit.into())
}
fn chart_maximum(metric: Metric, values: impl Iterator<Item = f64>) -> f64 {
    if matches!(metric, Metric::Cpu | Metric::Gpu) {
        return 100.0;
    }
    let peak = values.fold(0.0, f64::max);
    if peak == 0.0 {
        return 1024.0;
    }
    let target = peak * 1.12;
    let unit = [3, 2, 1]
        .into_iter()
        .map(|power| 1024_f64.powi(power))
        .find(|unit| target >= *unit)
        .unwrap_or(1.0);
    let magnitude = 10_f64.powf((target / unit).log10().floor()) * unit;
    [1.0, 2.0, 5.0, 10.0]
        .into_iter()
        .map(|step| step * magnitude)
        .find(|cap| *cap >= target)
        .unwrap_or(target)
}
fn plot_value(frame: &Frame, metric: Metric, id: Option<&str>, writing: bool) -> Option<f64> {
    if metric == Metric::Disk {
        return frame
            .disks
            .iter()
            .find(|d| Some(d.id.as_str()) == id)?
            .rate
            .map(|(read, write)| if writing { write } else { read });
    }
    graph_value(frame, metric, id)
}
fn write_color() -> ColorF {
    ColorF::new(0.42, 0.59, 0.83, 1.0)
}
type PlotSample = (std::time::Instant, Option<f64>, bool);

// Segment boundaries are recorded by the sampler. Changing the refresh setting
// must never reinterpret already collected history as missing samples.
fn graph_segments(
    samples: &[PlotSample],
    at: std::time::Instant,
    rect: Rect,
    maximum: f64,
) -> Vec<Vec<Vector2>> {
    let mut segments = Vec::new();
    let mut segment = Vec::new();
    for &(time, value, break_before) in samples {
        if (value.is_none() || break_before) && !segment.is_empty() {
            segments.push(std::mem::take(&mut segment));
        }
        if let Some(value) = value {
            let age = at.saturating_duration_since(time).as_secs_f32();
            segment.push(Vector2::new(
                rect.right - (rect.right - rect.left) * age.min(60.0) / 60.0,
                rect.bottom - (rect.bottom - rect.top) * (value / maximum).clamp(0.0, 1.0) as f32,
            ));
        }
    }
    if !segment.is_empty() {
        segments.push(segment);
    }
    segments
}

fn graph_series(
    target: &DrawPass<'_>,
    samples: &[PlotSample],
    at: std::time::Instant,
    rect: Rect,
    maximum: f64,
    stroke: &Brush,
    area: &Brush,
) -> windows::core::Result<()> {
    let segments = graph_segments(samples, at, rect, maximum);
    for segment in &segments {
        draw_segment(target, segment, rect.bottom, stroke, area)?;
    }
    if samples.last().is_some_and(|(_, value, _)| value.is_some())
        && let Some(&point) = segments.last().and_then(|segment| segment.last())
    {
        target.fill_ellipse(&Ellipse::circle(point, 2.0), stroke);
    }
    Ok(())
}

fn draw_segment(
    target: &DrawPass<'_>,
    points: &[Vector2],
    baseline: f32,
    accent: &Brush,
    area: &Brush,
) -> windows::core::Result<()> {
    if points.len() < 2 {
        return Ok(());
    }
    let polygon = std::iter::once(Vector2::new(points[0].x, baseline))
        .chain(points.iter().copied())
        .chain(std::iter::once(Vector2::new(
            points.last().unwrap().x,
            baseline,
        )));
    target.fill_polygon(polygon, area)?;
    for pair in points.windows(2) {
        target.draw_line(pair[0], pair[1], accent, 1.6);
    }
    Ok(())
}
fn color(metric: Metric, dark: bool) -> ColorF {
    let (r, g, b) = match metric {
        Metric::Cpu => {
            if dark {
                (0.34, 0.66, 0.98)
            } else {
                (0.12, 0.44, 0.80)
            }
        }
        Metric::Memory => {
            if dark {
                (0.70, 0.55, 0.96)
            } else {
                (0.55, 0.34, 0.80)
            }
        }
        Metric::Gpu => {
            if dark {
                (0.23, 0.75, 0.78)
            } else {
                (0.04, 0.53, 0.57)
            }
        }
        Metric::Network => {
            if dark {
                (0.29, 0.76, 0.53)
            } else {
                (0.08, 0.54, 0.34)
            }
        }
        Metric::Disk => {
            if dark {
                (0.93, 0.69, 0.34)
            } else {
                (0.69, 0.42, 0.12)
            }
        }
    };
    ColorF::new(r, g, b, 1.0)
}

fn selected_device<'a>(
    frame: &'a Frame,
    metric: Metric,
    options: &Preferences,
) -> Option<&'a Device> {
    let (devices, selected) = match metric {
        Metric::Network => (&frame.networks, &options.network),
        Metric::Disk => (&frame.disks, &options.disk),
        Metric::Gpu => (&frame.gpus, &options.gpu),
        _ => return None,
    };
    if let Some(id) = selected {
        let id = if metric == Metric::Gpu {
            crate::gpu::resolve(id)
        } else {
            id.clone()
        };
        return devices.iter().find(|d| d.id == id);
    }
    match metric {
        Metric::Network => devices.iter().max_by(|a, b| {
            let score = |d: &Device| d.rate.map_or(0.0, |(a, b)| a + b);
            score(a).total_cmp(&score(b)).then_with(|| b.id.cmp(&a.id))
        }),
        Metric::Gpu => devices
            .iter()
            .max_by_key(|d| (d.usage.is_some() || d.used.is_some(), d.total)),
        Metric::Disk => {
            let drive = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into());
            devices
                .iter()
                .find(|d| d.id.trim_end_matches('\\').eq_ignore_ascii_case(&drive))
                .or_else(|| devices.first())
        }
        _ => None,
    }
}
fn presentation(frame: &Frame, metric: Metric, device: Option<&Device>) -> (String, String) {
    let missing = crate::i18n::text("monitor-unavailable");
    let waiting = crate::i18n::text("monitor-collecting");
    let percent = |value: Option<f64>| value.map_or_else(|| "—".into(), |v| format!("{v:.0}%"));
    match metric {
        Metric::Cpu => (
            percent(frame.cpu),
            if frame.cpu.is_some() {
                crate::i18n::text("monitor-total-cpu")
            } else {
                waiting
            }
            .into(),
        ),
        Metric::Memory => frame.memory.map_or_else(
            || ("—".into(), missing.into()),
            |(used, total)| {
                (
                    percent(Some(used as f64 / total as f64 * 100.0)),
                    format!("{} / {}", bytes(used as f64), bytes(total as f64)),
                )
            },
        ),
        Metric::Gpu => device.map_or_else(
            || ("—".into(), missing.into()),
            |d| {
                let detail = d.used.map_or_else(
                    || {
                        if d.usage.is_some() {
                            d.name.clone()
                        } else {
                            format!("{missing} · {}", d.name)
                        }
                    },
                    |used| {
                        if d.total > 0 {
                            format!(
                                "{} / {} · {}",
                                bytes(used as f64),
                                bytes(d.total as f64),
                                d.name
                            )
                        } else {
                            format!("{} · {}", bytes(used as f64), d.name)
                        }
                    },
                );
                (percent(d.usage), detail)
            },
        ),
        Metric::Network | Metric::Disk => device.map_or_else(
            || ("—".into(), missing.into()),
            |d| {
                let (left, right) = if metric == Metric::Network {
                    ("↓", "↑")
                } else {
                    (
                        crate::i18n::text("monitor-read"),
                        crate::i18n::text("monitor-write"),
                    )
                };
                d.rate.map_or_else(
                    || ("—".into(), format!("{} · {waiting}", d.name)),
                    |(a, b)| {
                        (
                            format!("{left} {}/s", bytes(a)),
                            format!("{right} {}/s · {}", bytes(b), d.name),
                        )
                    },
                )
            },
        ),
    }
}
fn graph_value(frame: &Frame, metric: Metric, id: Option<&str>) -> Option<f64> {
    match metric {
        Metric::Cpu => frame.cpu,
        Metric::Gpu => frame.gpus.iter().find(|d| Some(d.id.as_str()) == id)?.usage,
        Metric::Network => frame
            .networks
            .iter()
            .find(|d| Some(d.id.as_str()) == id)?
            .rate
            .map(|(a, b)| a + b),
        Metric::Disk => frame
            .disks
            .iter()
            .find(|d| Some(d.id.as_str()) == id)?
            .rate
            .map(|(a, b)| a + b),
        Metric::Memory => None,
    }
}
fn bytes(value: f64) -> String {
    for (power, unit) in [(3, "GiB"), (2, "MiB"), (1, "KiB")] {
        let scale = 1024_f64.powi(power);
        if value >= scale {
            return format!("{:.1} {unit}", value / scale);
        }
    }
    format!("{value:.0} B")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn compact_dashboard_flows_in_configured_order_without_overlaps_or_empty_rows() {
        fn check(metrics: &mut Vec<Metric>, remaining: &[Metric]) {
            for (width, height) in [(300.0, 240.0), (320.0, 280.0), (500.0, 240.0)] {
                let bounds = Rect::from_xywh(10.0, 56.0, width - 20.0, height - 90.0);
                let tiles = compact_tiles(metrics, bounds);
                assert_eq!(tiles.len(), metrics.len());
                let rows = usize::from(!tiles.is_empty())
                    + tiles
                        .windows(2)
                        .filter(|pair| pair[0].top != pair[1].top)
                        .count();
                assert!(rows <= 2);
                for (index, tile) in tiles.iter().enumerate() {
                    assert!(tile.right > tile.left && tile.bottom > tile.top);
                    assert!(tile.left >= bounds.left && tile.top >= bounds.top);
                    assert!(
                        tile.right <= bounds.right + 0.01 && tile.bottom <= bounds.bottom + 0.01
                    );
                    if let Some(next) = tiles.get(index + 1) {
                        assert!(
                            next.top >= tile.bottom
                                || (next.top == tile.top && next.left >= tile.right)
                        );
                    }
                    if tiles.get(index + 1).is_none_or(|next| next.top != tile.top) {
                        assert!((tile.right - bounds.right).abs() < 0.01);
                    }
                }
            }
            for (index, metric) in remaining.iter().enumerate() {
                metrics.push(*metric);
                let mut rest = remaining.to_vec();
                rest.remove(index);
                check(metrics, &rest);
                metrics.pop();
            }
        }
        let metrics = Preferences::default().metrics;
        let tiles = compact_tiles(&metrics, Rect::from_xywh(4.0, 50.0, 312.0, 202.0));
        assert_eq!(tiles[0].top, tiles[2].top);
        assert_eq!(tiles[3].top, tiles[4].top);
        assert!(tiles[3].top > tiles[0].bottom);
        check(&mut Vec::new(), &metrics);
    }

    #[test]
    fn compact_dashboard_renders_sample_values_and_limits_at_native_scales() {
        use canvas_core::Interface as _;
        use windows::core::Interface;
        let device = windows_canvas::GpuDevice::new_warp().unwrap();
        let mut drawing = Drawing::new().unwrap();
        drawing.was_paused = true;
        let device_reading = |id: &str, rate, usage| Device {
            id: id.into(),
            name: id.into(),
            rate,
            usage,
            used: None,
            total: 0,
        };
        let orders = [
            Preferences::default().metrics,
            vec![
                Metric::Disk,
                Metric::Cpu,
                Metric::Memory,
                Metric::Network,
                Metric::Gpu,
            ],
        ];
        for (order, metrics) in orders.into_iter().enumerate() {
            for (width, height) in [(320_i32, 280_i32), (300, 240)] {
                for dark in [false, true] {
                    for scale in [1.0, 1.5, 2.0] {
                        for limits in [false, true] {
                            drawing.snapshot = Arc::new(Snapshot {
                                frame: Arc::new(Frame {
                                    cpu: Some(if limits { 100.0 } else { 13.0 }),
                                    memory: Some((if limits { 0 } else { 8 }, 16)),
                                    gpus: vec![device_reading(
                                        "gpu",
                                        None,
                                        if limits { None } else { Some(15.0) },
                                    )],
                                    networks: vec![device_reading(
                                        "network",
                                        Some((108.0 * 1024.0, 5.5 * 1024.0)),
                                        None,
                                    )],
                                    disks: vec![device_reading(
                                        "disk",
                                        Some((0.0, 282.0 * 1024.0)),
                                        None,
                                    )],
                                    ..Default::default()
                                }),
                                ..Default::default()
                            });
                            let options = Preferences {
                                metrics: metrics.clone(),
                                compact: true,
                                paused: true,
                                ..Default::default()
                            };
                            let bitmap = device
                                .create_render_target(
                                    (width as f32 * scale) as u32,
                                    (height as f32 * scale) as u32,
                                )
                                .unwrap();
                            let mut result = Ok(());
                            bitmap.draw(|session| {
                                let ptr = session.raw().as_raw();
                                let native = unsafe { windows::Win32::Graphics::Direct2D::ID2D1DeviceContext::from_raw_borrowed(&ptr).unwrap() };
                                unsafe { native.SetDpi(96.0 * scale, 96.0 * scale); }
                                let target = DrawPass::new(session.raw(), native);
                                let background = if dark { 0.12549 } else { 1.0 };
                                target.clear(ColorF::new(background, background, background, 1.0));
                                let ink = if dark { 1.0 } else { 0.0 };
                                let text = target.create_solid_brush(ColorF::new(ink, ink, ink, 1.0)).unwrap();
                                target.clipped_text("系统监控", &drawing.compact_center, &Rect::from_xywh(0.0, 10.0, width as f32, 30.0), &text);
                                result = drawing.paint(&target, &options, Rect::from_xywh(6.0, 56.0, (width - 12) as f32, (height - 62) as f32), &text, &text, dark, ink);
                                Ok(())
                            }).unwrap();
                            result.unwrap();
                            let pixels = bitmap.read_pixels().unwrap();
                            assert!(
                                pixels.chunks_exact(4).any(|p| p[0] != p[1] || p[1] != p[2]),
                                "colored gauges/rate markers missing"
                            );
                            if std::env::var_os("LUCIDDESK_TEST_EXPORT_SNAPSHOTS").is_some()
                                && scale == 1.0
                                && !limits
                            {
                                let mut bmp = vec![0_u8; 54];
                                bmp[0..2].copy_from_slice(b"BM");
                                bmp[2..6]
                                    .copy_from_slice(&(54 + pixels.len() as u32).to_le_bytes());
                                bmp[10..14].copy_from_slice(&54_u32.to_le_bytes());
                                bmp[14..18].copy_from_slice(&40_u32.to_le_bytes());
                                bmp[18..22].copy_from_slice(&width.to_le_bytes());
                                bmp[22..26].copy_from_slice(&(-height).to_le_bytes());
                                bmp[26..28].copy_from_slice(&1_u16.to_le_bytes());
                                bmp[28..30].copy_from_slice(&32_u16.to_le_bytes());
                                bmp.extend_from_slice(&pixels);
                                let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                                    .join("../../target")
                                    .join(format!(
                                        "monitor-dashboard-{order}-{width}-{}.bmp",
                                        if dark { "dark" } else { "light" }
                                    ));
                                std::fs::write(path, bmp).unwrap();
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn history_keeps_valid_delayed_and_mixed_interval_samples_connected() {
        use std::time::{Duration, Instant};
        let start = Instant::now();
        // Five-second history followed by a faster refresh and a short delay:
        // none of these valid measurements is a pause or a missing reading.
        let samples: Vec<_> = [0, 5000, 10000, 11000, 13500, 14500]
            .into_iter()
            .map(|ms| (start + Duration::from_millis(ms), Some(10.0), false))
            .collect();
        let segments = graph_segments(
            &samples,
            samples.last().unwrap().0,
            Rect::from_xywh(0.0, 0.0, 600.0, 100.0),
            100.0,
        );
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].len(), 6);
        // Preserve real time spacing; no fabricated samples or zeroes.
        assert!((segments[0][1].x - segments[0][0].x - 50.0).abs() < 0.01);
        assert!((segments[0][4].x - segments[0][3].x - 25.0).abs() < 0.01);
    }

    #[test]
    fn history_still_breaks_for_missing_readings_and_explicit_pause_boundaries() {
        use std::time::{Duration, Instant};
        let start = Instant::now();
        let samples: Vec<_> = [
            (Some(10.0), true),
            (Some(20.0), false),
            (None, false),
            (Some(30.0), false),
            (Some(40.0), false),
            (Some(50.0), true),
            (Some(60.0), false),
        ]
        .into_iter()
        .enumerate()
        .map(|(i, (value, boundary))| (start + Duration::from_secs(i as u64), value, boundary))
        .collect();
        let segments = graph_segments(
            &samples,
            samples.last().unwrap().0,
            Rect::from_xywh(0.0, 0.0, 600.0, 100.0),
            100.0,
        );
        assert_eq!(segments.iter().map(Vec::len).collect::<Vec<_>>(), [2, 2, 2]);
    }

    #[test]
    fn axes_keep_percentages_absolute_and_disk_directions_separate() {
        assert_eq!(chart_maximum(Metric::Cpu, [2.0, 3.0].into_iter()), 100.0);
        assert_eq!(chart_maximum(Metric::Gpu, [90.0, 99.0].into_iter()), 100.0);
        let mut frame = Frame::default();
        frame.disks.push(Device {
            id: "disk".into(),
            name: "C:".into(),
            rate: Some((4096.0, 3072.0)),
            used: Some(100),
            total: 200,
            usage: None,
        });
        assert_eq!(
            plot_value(&frame, Metric::Disk, Some("disk"), false),
            Some(4096.0)
        );
        assert_eq!(
            plot_value(&frame, Metric::Disk, Some("disk"), true),
            Some(3072.0)
        );
        assert_eq!(
            chart_maximum(Metric::Disk, [4096.0, 3072.0].into_iter()),
            5120.0
        );
        assert_eq!(
            rate_parts(34.3 * 1024.0 * 1024.0),
            ("34.3".into(), "MiB/s".into())
        );
    }
    #[test]
    fn memory_uses_less_space_so_chart_cards_have_meaningful_height() {
        let metrics = [
            Metric::Cpu,
            Metric::Memory,
            Metric::Gpu,
            Metric::Network,
            Metric::Disk,
        ];
        let heights = standard_heights(&metrics, 500.0);
        assert!((heights.iter().sum::<f32>() - 500.0).abs() < 0.01);
        assert!(heights[1] <= 64.0);
        assert!(heights[0] - 38.0 >= 60.0);
        assert!(heights[2] - 52.0 >= 50.0);
        assert!(heights[4] - 77.0 >= 50.0);
    }
    #[test]
    fn disconnected_selected_device_never_silently_switches_to_another() {
        let mut frame = Frame::default();
        frame.networks.push(Device {
            id: "other".into(),
            name: "Other".into(),
            rate: Some((100.0, 200.0)),
            total: 0,
            used: None,
            usage: None,
        });
        let prefs = Preferences {
            network: Some("missing".into()),
            ..Default::default()
        };
        assert!(selected_device(&frame, Metric::Network, &prefs).is_none());
        assert_eq!(graph_value(&frame, Metric::Network, Some("missing")), None);
    }
    #[test]
    fn rates_use_binary_units_and_preserve_zero_as_a_real_reading() {
        assert_eq!(bytes(0.0), "0 B");
        assert_eq!(bytes(1024.0 * 1024.0), "1.0 MiB");
        assert_eq!(
            presentation(
                &Frame {
                    cpu: Some(0.0),
                    ..Default::default()
                },
                Metric::Cpu,
                None
            )
            .0,
            "0%"
        );
        assert_eq!(presentation(&Frame::default(), Metric::Cpu, None).0, "—");
    }
    #[test]
    fn automatic_gpu_prefers_live_counters_over_larger_unavailable_adapters() {
        let mut frame = Frame::default();
        frame.gpus.push(Device {
            id: "real".into(),
            name: "GPU".into(),
            rate: None,
            used: Some(1024),
            total: 4096,
            usage: Some(1.0),
        });
        frame.gpus.push(Device {
            id: "unavailable".into(),
            name: "GPU".into(),
            rate: None,
            used: None,
            total: 8192,
            usage: None,
        });
        assert_eq!(
            selected_device(&frame, Metric::Gpu, &Preferences::default())
                .unwrap()
                .id,
            "real"
        );
    }
}
