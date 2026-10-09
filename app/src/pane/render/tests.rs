#[test]
fn title_ellipsis_stays_stable_when_width_jitters_at_last_character() {
    use windows::Win32::Graphics::DirectWrite::{DWRITE_LINE_METRICS, IDWriteTextLayout};
    let _sta = crate::pane::test_support::apartment();
    let trimmed = |title: &windows_canvas::TextLayout| {
        let native: IDWriteTextLayout =
            super::super::native_graphics::native_interface(title.raw()).unwrap();
        let mut lines = [DWRITE_LINE_METRICS::default(); 1];
        let mut count = 0;
        unsafe {
            native
                .GetLineMetrics(Some(&mut lines), &raw mut count)
                .unwrap();
        }
        lines[0].isTrimmed.as_bool()
    };
    let mut renderer = Renderer::new().unwrap();
    for scale in [1.0, 1.25, 1.5, 2.0] {
        for text in ["新建分组", "Project 项目文件夹与资料", "tinyMediaManager"] {
            let mut boundary = None;
            for width in (8..=600).rev() {
                let title = renderer
                    .layout_title(text, (width as f32 + 0.01) / scale, scale)
                    .unwrap();
                if trimmed(&title) {
                    boundary = Some(width);
                    break;
                }
            }
            let boundary = boundary.expect("title must cross its actual trimming boundary");
            for offset in [1, 0, 2, 1, 0, 1, 2, 0] {
                let title = renderer
                    .layout_title(text, ((boundary + offset) as f32 + 0.01) / scale, scale)
                    .unwrap();
                assert!(
                    trimmed(&title),
                    "ellipsis toggled at scale {scale}, offset {offset}"
                );
            }
            let title = renderer
                .layout_title(text, (boundary as f32 + 4.01) / scale, scale)
                .unwrap();
            assert!(
                !trimmed(&title),
                "full title must return when enough space is available"
            );
        }
    }
}

#[test]
fn title_trimming_does_not_reverse_while_shrinking() {
    use windows::Win32::Graphics::DirectWrite::{DWRITE_LINE_METRICS, IDWriteTextLayout};
    let _sta = crate::pane::test_support::apartment();
    let mut renderer = Renderer::new().unwrap();
    for scale in [1.0, 1.25, 1.5, 2.0] {
        for text in ["新建分组", "Project 项目文件夹与资料", "tinyMediaManager"] {
            let mut was_trimmed = false;
            let mut previous_end = u32::MAX;
            for width in (8..=600).rev() {
                let title = renderer
                    .layout_title(text, width as f32 / scale, scale)
                    .unwrap();
                let native: IDWriteTextLayout =
                    super::super::native_graphics::native_interface(title.raw()).unwrap();
                let mut lines = [DWRITE_LINE_METRICS::default(); 1];
                let mut count = 0;
                unsafe {
                    native
                        .GetLineMetrics(Some(&mut lines), &raw mut count)
                        .unwrap();
                }
                let trimmed = lines[0].isTrimmed.as_bool();
                assert!(
                    !was_trimmed || trimmed,
                    "ellipsis reverted at {width}px, scale {scale}"
                );
                let end = title
                    .hit_test_point(Vector2::new(title.max_size().0 - 0.25, HEADER / 2.0))
                    .text_position;
                assert!(
                    end <= previous_end,
                    "shrinking revealed characters at {width}px"
                );
                was_trimmed = trimmed;
                previous_end = end;
            }
            assert!(was_trimmed);
        }
    }
}
#[test]
fn list_view_columns_render_and_share_scrolled_hit_geometry() {
    let _sta = crate::pane::test_support::apartment();
    let mut model = sample_model();
    model.folder = Some(std::path::PathBuf::from(r"C:\Documents"));
    assert!(!model.header_button_enabled(2));
    assert!(!model.header_button_enabled(3));
    model.folder_navigation = [true, true];
    assert!(model.header_button_enabled(2));
    assert!(model.header_button_enabled(3));
    model.list_view = true;
    model.items[0].label = "项目进度报告.txt".into();
    model.items[0].details = super::super::ItemDetails {
        kind: "文本文档".into(),
        modified: "2026/09/12 16:30".into(),
        size: Some(1536),
        ..Default::default()
    };
    model.items = vec![model.items[0].clone(); 20];
    let mut renderer = Renderer::new().unwrap();
    for scale in [1.0, 1.25, 1.5, 2.0] {
        model.scroll = 3;
        let grid = model.grid(480.0, 300.0);
        let (x, y) = model.cell(grid, 3);
        assert_eq!(grid.columns, 1);
        assert_eq!(model.hit(grid, x + 4.0, y + 10.0, scale), Some(3));
        assert_eq!(
            model.hit(grid, x + grid.cell_width - 4.0, y + 10.0, scale),
            Some(3)
        );
        assert_eq!(model.hit(grid, x + 20.0, y - 2.0, scale), None);
        assert_eq!(
            model.hit(grid, x + 20.0, y + grid.cell_height + 2.0, scale),
            Some(4)
        );
        let width = (480.0 * scale) as u32;
        let height = (300.0 * scale) as u32;
        let pixels = renderer.pixels(width, height, scale, &model).unwrap();
        let mut blank = model.clone();
        let bar = super::super::scrollbar::Bar::for_model(&model, 480.0, 300.0).unwrap();
        let mut hovered = model.clone();
        hovered.scrollbar.hovered = true;
        hovered.scrollbar.expansion = 1.0;
        let active = renderer.pixels(width, height, scale, &hovered).unwrap();
        assert!(((bar.top * scale) as u32..((bar.top + bar.height) * scale) as u32).any(|row| {
            ((bar.left * scale) as u32..((bar.left + super::super::scrollbar::Bar::WIDTH) * scale) as u32).any(|col| {
                let at = ((row * width + col) * 4) as usize;
                pixels[at..at + 4] != active[at..at + 4]
            })
        }), "scrollbar hover must be visible at scale {scale}");
        for item in &mut blank.items {
            item.label.clear();
            item.details = Default::default();
        }
        let empty = renderer.pixels(width, height, scale, &blank).unwrap();
        let columns = super::super::layout::list_columns(grid.cell_width);
        for column in 0..4 {
            let changed = (((y + 2.0) * scale) as u32
                ..((y + grid.cell_height - 2.0) * scale) as u32)
                .any(|row| {
                    (((x + columns[column]) * scale) as u32
                        ..((x + columns[column + 1] - 8.0) * scale) as u32)
                        .any(|col| {
                            let at = ((row * width + col) * 4) as usize;
                            pixels[at..at + 4] != empty[at..at + 4]
                        })
                });
            assert!(
                changed,
                "column {column} must contain rendered text at scale {scale}"
            );
        }
    }
}
#[test]
fn desktop_list_renders_full_width_names_and_row_drag_preview() {
    let _sta = crate::pane::test_support::apartment();
    let mut model = sample_model();
    model.list_view = true;
    model.items[0].label = "普通分组中较长的文件名称 — desktop document.txt".into();
    model.items = vec![model.items[0].clone(); 20];
    model.scroll = 3;
    let mut renderer = Renderer::new().unwrap();
    for scale in [1.0, 1.25, 1.5, 2.0] {
        let grid = model.grid(480.0, 300.0);
        let (x, y) = model.cell(grid, 3);
        assert_eq!(model.hit(grid, x + grid.cell_width - 2.0, y + 10.0, scale), Some(3));
        assert_eq!(model.hit(grid, x + 20.0, y - 2.0, scale), None);
        let pixels = renderer.pixels((480.0 * scale) as u32, (300.0 * scale) as u32, scale, &model).unwrap();
        let mut blank = model.clone();
        for item in &mut blank.items { item.label.clear(); }
        let empty = renderer.pixels((480.0 * scale) as u32, (300.0 * scale) as u32, scale, &blank).unwrap();
        assert_ne!(pixels, empty);
        let icon = assets::Pixels { width: 1, height: 1, data: vec![255; 4] };
        let preview = super::super::drag_drop::image::list_item_pixels(&icon, &model.items[0].label, grid, scale).unwrap();
        assert_eq!(preview.height, (grid.cell_height * scale).round() as u32);
        assert!(preview.data.chunks_exact(4).any(|p| p[3] > 0));
    }
}

#[test]
fn grid_scale_changes_icon_and_text_size_together() {
    let _sta = crate::pane::test_support::apartment();
    let mut model = sample_model();
    let mut renderer = Renderer::new().unwrap();
    for dpi_scale in [1.0, 1.25, 1.5, 2.0] {
        let mut previous_text_height = 0.0;
        for percent in [50.0, 100.0, 150.0, 200.0] {
            model.options.grid_scale = percent;
            let grid = model.grid(600.0, 500.0);
            assert_eq!(grid.icon_size, model.icon_size * percent / 100.0);
            let (_, text_height) = super::super::label::layout_scaled(
                "Desktop", (grid.cell_width * dpi_scale).round() as u32,
                (96.0 * dpi_scale).round() as u32, 2, grid.text_scale,
            ).unwrap();
            assert!(text_height > previous_text_height, "glyph height must scale at {percent}%");
            previous_text_height = text_height;
            renderer.pixels((600.0 * dpi_scale) as u32, (500.0 * dpi_scale) as u32, dpi_scale, &model).unwrap();
            let icon = assets::Pixels { width: 1, height: 1, data: vec![255; 4] };
            let preview = super::super::drag_drop::image::item_pixels(&icon, "Desktop", grid, dpi_scale).unwrap();
            assert!(preview.height as f32 > grid.icon_size * dpi_scale);
        }
    }
}
use super::*;

#[test]
fn icon_pixels_remain_sharp_at_fractional_dpi_and_invalidate_size_cache() {
    let _apartment = crate::pane::test_support::apartment();
    let mut model = sample_model();
    let mut data = vec![0; 48 * 48 * 4];
    for y in 4..44usize {
        for x in 4..44usize {
            let value = if (x / 3 + y / 3) % 2 == 0 { 255 } else { 0 };
            data[(y * 48 + x) * 4..(y * 48 + x + 1) * 4]
                .copy_from_slice(&[value, value, value, 255]);
        }
    }
    model.items[0].image = Some(Arc::new(assets::Pixels {
        width: 48,
        height: 48,
        data,
    }));
    model.clear_selection();
    model.hovered_item = None;
    let mut renderer = Renderer::new().unwrap();
    for scale in [1.0, 1.25, 1.5, 2.0, 1.0] {
        let pixels = renderer.pixels(600, 420, scale, &model).unwrap();
        let grid = model.grid(600.0 / scale, 420.0 / scale);
        let (x, y) = model.cell(grid, 0);
        let size = (grid.icon_size * scale).round() as u32;
        let expected =
            assets::resample(model.items[0].image.as_ref().unwrap(), size, size).unwrap();
        let left =
            ((x + (grid.cell_width - size as f32 / scale) / 2.0) * scale).round() as usize;
        let top =
            ((y + 2.0 + (grid.icon_size - size as f32 / scale) / 2.0) * scale).round() as usize;
        for iy in 0..size as usize {
            for ix in 0..size as usize {
                for c in 0..4 {
                    let actual = pixels[((top + iy) * 600 + left + ix) * 4 + c];
                    let expected = expected.data[(iy * size as usize + ix) * 4 + c];
                    assert!(
                        actual.abs_diff(expected) <= 1,
                        "icon was filtered a second time at DPI {scale}"
                    );
                }
            }
        }
    }
}

#[test]
fn canvas_flyout_retains_transparency_and_hover_after_resize() {
    let _apartment = crate::pane::test_support::apartment();
    let mut renderer = Renderer::new().unwrap();
    let entries = [super::super::menu::Entry {
        id: 1,
        label: "返回上个文件夹",
        icon: "",
        trailing: "Alt + ←",
        children: Vec::new(),
    }];
    for (width, height, scale) in [
        (240, 48, 1.0),
        (360, 72, 1.5),
        (480, 96, 2.0),
        (240, 48, 1.0),
    ] {
        let pixels = renderer
            .flyout(width, height, scale, &entries, None, true, true)
            .unwrap();
        let selected = renderer
            .flyout(width, height, scale, &entries, Some(0), true, true)
            .unwrap();
        let at = (((12.0 * scale) as u32 * width + (12.0 * scale) as u32) * 4) as usize;
        assert_eq!(pixels[at + 3], 0);
        assert!(selected[at + 3] > 0 && selected[at + 3] < 255);
        let opaque = renderer
            .flyout(width, height, scale, &entries, None, false, false)
            .unwrap();
        assert_eq!(opaque[at + 3], 255);
        for (x, y) in [(0, 0), (width - 1, 0), (0, height - 1), (width - 1, height - 1)] {
            assert_eq!(opaque[((y * width + x) * 4 + 3) as usize], 0);
        }
    }
}
use crate::pane::{Item, assets::Pixels};
use luciddesk_core::ShellIdentity;
use std::sync::Arc;

#[test]
fn tab_strip_renders_at_supported_dpi_and_widths() {
    let _sta = crate::pane::test_support::apartment();
    let mut model = sample_model();
    model.tabs = ["工作", "项目资料", "下载与归档"].into_iter().enumerate()
        .map(|(at, title)| (luciddesk_core::PanelId::new(at as u64 + 1), title.into())).collect();
    model.active_tab = luciddesk_core::PanelId::new(2);
    let mut renderer = Renderer::new().unwrap();
    for scale in [1.0, 1.5, 2.0] {
        for width in [260, 420, 800] {
            for dark in [false, true] {
                model.dark = dark;
                let w = (width as f32 * scale) as u32;
                let h = (300.0 * scale) as u32;
                let pixels = renderer.pixels(w, h, scale, &model).unwrap();
                assert_eq!(pixels.len(), (w * h * 4) as usize);
                if let Some((id, _)) = super::super::tabs::strip(&model, width as f32).into_iter().find(|(id, _)| *id != model.active_tab) {
                    let mut hovered = model.clone();
                    hovered.hovered_tab = Some(id);
                    let hover_pixels = renderer.pixels(w, h, scale, &hovered).unwrap();
                    assert_ne!(pixels, hover_pixels);
                    let body = ((HEADER * scale).ceil() as u32 * w * 4) as usize;
                    assert_eq!(&pixels[body..], &hover_pixels[body..], "tab hover must not change the material or pane body");
                }
                let mut title_changed = model.clone();
                title_changed.title = "这个旧标题不应绘制".into();
                assert_eq!(pixels, renderer.pixels(w, h, scale, &title_changed).unwrap());
                let mut blank = model.clone();
                for (_, title) in &mut blank.tabs { title.clear(); }
                assert_ne!(pixels, renderer.pixels(w, h, scale, &blank).unwrap());
                if std::env::var_os("LUCIDDESK_TEST_EXPORT_SNAPSHOTS").is_some() && scale == 1.0 && width == 420 && dark {
                    let mut bmp = vec![0u8; 54];
                    bmp[..2].copy_from_slice(b"BM");
                    bmp[2..6].copy_from_slice(&(54u32 + pixels.len() as u32).to_le_bytes());
                    bmp[10..14].copy_from_slice(&54u32.to_le_bytes());
                    bmp[14..18].copy_from_slice(&40u32.to_le_bytes());
                    bmp[18..22].copy_from_slice(&(w as i32).to_le_bytes());
                    bmp[22..26].copy_from_slice(&(-(h as i32)).to_le_bytes());
                    bmp[26..28].copy_from_slice(&1u16.to_le_bytes());
                    bmp[28..30].copy_from_slice(&32u16.to_le_bytes());
                    bmp.extend_from_slice(&pixels);
                    let output = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/tabs-preview.bmp");
                    std::fs::write(output, bmp).unwrap();
                }
            }
        }
    }
}

#[test]
fn placeholders_render_without_textures_then_yield_to_loaded_icons() {
    let _apartment = crate::pane::test_support::apartment();
    for dark in [false, true] {
        let mut model = sample_model();
        model.dark = dark;
        model.native_material = false;
        model.title = "Placeholder preview".into();
        let loaded = model.items[0].image.take();
        model.items[0].label = "Document".into();
        let mut folder = model.items[0].clone();
        folder.identity = ShellIdentity::Namespace { parsing_name: "test:folder".into() };
        folder.label = "Folder".into();
        folder.details.folder = true;
        model.items.push(folder);
        let mut renderer = Renderer::new().unwrap();
        for scale in [1.0, 1.5, 2.0] {
            let pixels = renderer.pixels((320.0 * scale) as u32, (180.0 * scale) as u32, scale, &model).unwrap();
            assert!(renderer.images.is_empty(), "placeholders must not upload textures");
            if scale == 1.0 && std::env::var_os("LUCIDDESK_TEST_EXPORT_SNAPSHOTS").is_some() {
                let mut bmp = vec![0u8; 54];
                bmp[0..2].copy_from_slice(b"BM");
                bmp[2..6].copy_from_slice(&(54 + pixels.len() as u32).to_le_bytes());
                bmp[10..14].copy_from_slice(&54u32.to_le_bytes());
                bmp[14..18].copy_from_slice(&40u32.to_le_bytes());
                bmp[18..22].copy_from_slice(&320i32.to_le_bytes());
                bmp[22..26].copy_from_slice(&(-180i32).to_le_bytes());
                bmp[26..28].copy_from_slice(&1u16.to_le_bytes());
                bmp[28..30].copy_from_slice(&32u16.to_le_bytes());
                bmp.extend(pixels);
                std::fs::write(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../target/placeholder-{dark}.bmp")), bmp).unwrap();
            }
        }
        model.items[0].image = loaded;
        renderer.pixels(320, 180, 1.0, &model).unwrap();
        assert_eq!(renderer.images.len(), 1);
    }
}

fn sample_model() -> GroupModel {
    GroupModel {
        focused: true,
        backdrop: luciddesk_core::Backdrop::Acrylic,
        native_material: true,
        items: vec![Item {
            details: Default::default(),
            identity: ShellIdentity::Namespace {
                parsing_name: "test:opaque-icon".into(),
            },
            label: "Test".into(),
            image: Some(Arc::new(Pixels {
                width: 16,
                height: 16,
                data: [80, 100, 200, 255].repeat(16 * 16),
            })),
        }],
        ..super::super::tests::test_model("透明度验证")
    }
}

#[test]
fn pane_border_and_corners_can_be_disabled_independently() {
    let _sta = crate::pane::test_support::apartment();
    let mut renderer = Renderer::new().unwrap();
    let mut model = sample_model();
    model.items.clear();
    model.title.clear();
    model.native_material = true;
    let bordered = renderer.pixels(320, 200, 1.0, &model).unwrap();
    model.options.border = false;
    let borderless = renderer.pixels(320, 200, 1.0, &model).unwrap();
    assert!(bordered[160 * 4 + 3] > borderless[160 * 4 + 3]);
    model.native_material = false;
    let rounded = renderer.pixels(320, 200, 1.0, &model).unwrap();
    model.options.corner_radius = 0.0;
    let square = renderer.pixels(320, 200, 1.0, &model).unwrap();
    let corner = (320 + 1) * 4 + 3;
    assert!(square[corner] > rounded[corner]);
}

#[test]
fn item_hit_stops_at_visible_highlight_across_dpi_and_scroll() {
    let _sta = crate::pane::test_support::apartment();
    let mut model = sample_model();

    model.items.push(sample_model().items.remove(0));
    for scale in [1.0, 1.25, 1.5, 2.0] {
        for scroll in [0, 1] {
            model.scroll = scroll;
            for text in ["Termius", "A longer name that wraps onto two lines"] {
                model.items[scroll].label = text.into();
                let grid = model.grid(112.0, 250.0);
                let (x, y) = grid.cell(scroll, scroll);
                let label = super::super::label::raster(
                    text,
                    (grid.cell_width * scale).round() as u32,
                    (96.0 * scale).round() as u32,
                    2,
                )
                .unwrap();
                let bottom = y
                    + (grid.icon_size
                        + crate::pane::layout::LABEL_OFFSET
                        + 1.0
                        + label.text_height as f32 / scale)
                        .min(grid.cell_height - 2.0);
                let center = x + grid.cell_width / 2.0;
                assert_eq!(
                    model.hit(grid, center, bottom - 1.0 / scale, scale),
                    Some(scroll)
                );
                assert_eq!(model.hit(grid, center, bottom, scale), None);
                assert_eq!(
                    model.hit(grid, center, y + grid.cell_height - 1.0, scale),
                    None
                );
                assert_eq!(model.hit(grid, x - 1.0, y + 10.0, scale), None);
                assert_eq!(model.hit(grid, x + grid.cell_width, y + 10.0, scale), None);
            }
        }
    }
}

#[test]
fn light_and_dark_text_contrast_without_changing_icon_pixels() {
    let _apartment = crate::pane::test_support::apartment();
    let mut model = sample_model();
    model.native_material = false;
    let mut renderer = Renderer::new().unwrap();
    for dark in [true, false] {
        model.dark = dark;
        let pixels = renderer.pixels(400, 240, 1.0, &model).unwrap();
        let at = |x: usize, y: usize| &pixels[(y * 400 + x) * 4..][..4];
        assert_eq!(at(59, 78), [80, 100, 200, 255]);
        assert_eq!(at(380, 200)[0] < 128, dark);
        let ink_present = (5..32).any(|y| {
            (106..294).any(|x| {
                let p = at(x, y)[0];
                if dark { p > 180 } else { p < 100 }
            })
        });
        assert!(ink_present, "Title must contrast with its background");
    }
}

#[test]
fn transparent_panel_protection_preserves_icons_and_rounded_edges() {
    let _apartment = crate::pane::test_support::apartment();
    let mut model = sample_model();
    model.options.text_protection = true;
    model.backdrop = luciddesk_core::Backdrop::Solid {
        color: 0xffffff,
        opacity: 0.0,
    };
    let mut renderer = Renderer::new().unwrap();
    for mode in [
        luciddesk_core::PanelText::Light,
        luciddesk_core::PanelText::Dark,
    ] {
        model.options.text = mode;
        let pixels = renderer.pixels(400, 240, 1.0, &model).unwrap();
        let at = |x: usize, y: usize| &pixels[(y * 400 + x) * 4..][..4];
        assert_eq!(at(59, 78), [80, 100, 200, 255]);
        assert!(at(380, 200)[3] > 0 && at(380, 200)[3] < 255);
        assert_eq!(at(0, 0)[3], 0);
        assert_eq!(at(380, 200)[0] == 0, mode == luciddesk_core::PanelText::Light);
        model.options.text_protection = false;
        let unprotected = renderer.pixels(400, 240, 1.0, &model).unwrap();
        assert_eq!(unprotected[(200 * 400 + 380) * 4 + 3], 0);
        assert_eq!(
            &unprotected[(78 * 400 + 59) * 4..][..4],
            [80, 100, 200, 255]
        );
        model.options.text_protection = true;
    }
}

#[test]
fn background_alpha_does_not_dim_icons_at_multiple_scales() {
    let _apartment = crate::pane::test_support::apartment();
    let mut model = sample_model();
    let mut renderer = Renderer::new().unwrap();
    for scale in [1.0, 1.5, 2.0] {
        let width = (400.0 * scale) as u32;
        let pixels = renderer
            .pixels(width, (240.0 * scale) as u32, scale, &model)
            .unwrap();
        let at = |x: f32, y: f32| -> &[u8] {
            let index = (((y * scale) as u32 * width + (x * scale) as u32) * 4) as usize;
            &pixels[index..index + 4]
        };
        assert_eq!(
            at(59.0, 78.0),
            [80, 100, 200, 255],
            "icon retains its original opaque colors"
        );
        assert!(
            at(380.0, 200.0)[3] == 0,
            "content leaves the native backdrop unobstructed"
        );
        assert_eq!(at(0.0, 0.0)[3], 0, "outside rounded corner is transparent");
        model.native_material = false;
        let fallback = renderer
            .pixels(width, (240.0 * scale) as u32, scale, &model)
            .unwrap();
        let background =
            (((200.0 * scale) as u32 * width + (380.0 * scale) as u32) * 4) as usize;
        assert_eq!(
            fallback[background + 3],
            255,
            "unsupported materials have an opaque fallback"
        );
        model.native_material = true;
    }
}

#[test]
fn viewport_layout_matches_full_measurement_across_sizes_and_scroll_positions() {
    let _sta = crate::pane::test_support::apartment();
    let mut model = sample_model();
    let template = model.items[0].clone();
    for count in [1, 37, 257] {
        model.items = (0..count).map(|index| Item {
            label: ["short", "long filename with wrapping content", "项目文件名称与测试资料"][index % 3].into(),
            ..template.clone()
        }).collect();
        for percent in [50.0, 100.0, 200.0] {
            model.options.grid_scale = percent;
            for width in [180.0, 420.0] {
                for height in [44.0, 80.0, 240.0, 600.0] {
                    for scroll in [0, 2, count - 1] {
                        model.scroll = scroll;
                        let grid = model.grid(width, height);
                        let rows = model.row_contents(grid);
                        let available = height - super::super::layout::HEADER - super::super::layout::PADDING;
                        let old_visible = super::super::layout::fitting_rows(&rows[scroll.min(rows.len()-1)..], grid.cell_height, available);
                        let old_limit = (0..rows.len()).find(|start| super::super::layout::fitting_rows(&rows[*start..], grid.cell_height, available) >= rows.len()-start).unwrap_or(rows.len()-1);
                        assert_eq!(grid.visible_rows, old_visible);
                        assert_eq!(grid.scroll_limit, Some(old_limit));
                        let expected: Vec<_> = (0..count).filter(|index| {
                            let (_, y) = model.cell(grid, *index);
                            y + grid.cell_height > HEADER && y < height
                        }).collect();
                        let actual: Vec<_> = grid.visible_indices(scroll, HEADER, height, count).filter(|index| {
                            let (_, y) = model.cell(grid, *index);
                            y + grid.cell_height > HEADER && y < height
                        }).collect();
                        assert_eq!(actual, expected);
                    }
                }
            }
        }
    }
}

#[test]
fn selection_backgrounds_release_old_sizes_and_deselected_textures() {
    let _sta = crate::pane::test_support::apartment();
    let mut model = sample_model();
    model.list_view = true;
    model.selection.insert(0);
    let device = windows_canvas::GpuDevice::new_warp().unwrap();
    let mut renderer = Renderer::new().unwrap();
    let bitmap = canvas::Offscreen::new(&device, 640, 240).unwrap();
    for width in [320, 480, 640, 400] {
        renderer.paint(&bitmap.target, width, 240, 1.0, &model).unwrap();
        assert_eq!(renderer.states.len(), 1);
    }
    model.selection.clear();
    model.hovered_item = None;
    renderer.paint(&bitmap.target, 400, 240, 1.0, &model).unwrap();
    assert!(renderer.states.is_empty());
}

#[test]
fn same_size_upload_borrows_original_pixel_buffer() {
    let source = assets::Pixels { width: 32, height: 32, data: vec![255; 32 * 32 * 4] };
    let pixels = assets::resample(&source, 32, 32).unwrap();
    assert!(std::ptr::eq(pixels.data.as_ptr(), source.data.as_ptr()));
}

#[test]
fn identical_visible_icons_share_one_gpu_upload_and_changed_pixels_replace_it() {
    let _sta = crate::pane::test_support::apartment();
    let mut model = sample_model();
    let template = model.items[0].clone();
    model.items = (0..8).map(|index| Item {
        identity: ShellIdentity::Namespace { parsing_name: format!("shared:{index}") },
        ..template.clone()
    }).collect();
    let device = windows_canvas::GpuDevice::new_warp().unwrap();
    let mut renderer = Renderer::new().unwrap();
    for scale in [1.0, 1.5, 2.0] {
        let bitmap = canvas::Offscreen::new(&device, (600.0 * scale) as u32, (400.0 * scale) as u32).unwrap();
        renderer.paint(&bitmap.target, (600.0 * scale) as u32, (400.0 * scale) as u32, scale, &model).unwrap();
        assert_eq!(renderer.images.len(), 1, "shared pixels should upload only once");
    }
    let bitmap = canvas::Offscreen::new(&device, 600, 400).unwrap();
    Arc::make_mut(model.items[0].image.as_mut().unwrap()).data[0] ^= 1;
    renderer.paint(&bitmap.target, 600, 400, 1.0, &model).unwrap();
    assert_eq!(renderer.images.len(), 2);
    model.items.remove(0);
    renderer.paint(&bitmap.target, 600, 400, 1.0, &model).unwrap();
    assert_eq!(renderer.images.len(), 1);
}

#[test]
fn scrolling_releases_offscreen_icon_textures() {
    let _sta = crate::pane::test_support::apartment();
    let mut model = sample_model();
    let image = model.items[0].image.clone();
    model.items = (0..1000)
        .map(|index| Item {
            identity: ShellIdentity::Namespace {
                parsing_name: format!("test:icon-{index}"),
            },
            label: format!("Icon {index}"),
            image: image.as_ref().map(|image| Arc::new((**image).clone())),
            details: Default::default(),
        })
        .collect();
    let device = windows_canvas::GpuDevice::new().unwrap();
    let bitmap = canvas::Offscreen::new(&device, 400, 240).unwrap();
    let mut renderer = Renderer::new().unwrap();
    for row in (0..150).step_by(5) {
        model.scroll = row;
        renderer
            .paint(&bitmap.target, 400, 240, 1.0, &model)
            .unwrap();
        assert!(!renderer.images.is_empty());
        assert!(
            renderer.images.len() <= 32,
            "offscreen uploads accumulated: {}",
            renderer.images.len()
        );
    }
    model.scroll = 0;
    renderer
        .paint(&bitmap.target, 400, 240, 1.0, &model)
        .unwrap();
    assert!(
        renderer
            .images
            .contains_key(&(Arc::as_ptr(model.items[0].image.as_ref().unwrap()) as usize))
    );
}

#[test]
fn gpu_frames_preserve_colors_alpha_and_cached_images_across_resize() {
    let _apartment = crate::pane::test_support::apartment();
    let model = sample_model();
    // Exercise the actual swap-chain path, including buffer rotation and resize.
    // The test window stays hidden and never takes over Explorer.
    let window = windows_window::Window::new("LucidDesk GPU regression")
        .size(400, 240)
        .style(windows_sys::Win32::UI::WindowsAndMessaging::WS_POPUP)
        .ex_style(windows_sys::Win32::UI::WindowsAndMessaging::WS_EX_NOREDIRECTIONBITMAP)
        .create()
        .unwrap();
    {
        let mut surface = super::super::composition::Surface::new(
            windows::Win32::Foundation::HWND(window.hwnd().cast()),
        )
        .unwrap();
        let mut gpu = Renderer::new().unwrap();
        for (width, height) in [(400, 240), (400, 240), (500, 300), (400, 240)] {
            let target = surface.begin_frame(width, height).unwrap();
            gpu.paint(&target, width, height, 1.0, &model).unwrap();
            assert!(
                gpu.target.as_ref().unwrap().2.is_none(),
                "live rendering has no CPU frame bitmap"
            );
            assert_eq!(gpu.images.len(), 1);
            let pixels = surface.readback().unwrap();
            let grid = model.grid(width as f32, height as f32);
            let (x, y) = grid.cell(0, 0);
            let at =
                (((y + 20.0) as u32 * width + (x + grid.cell_width / 2.0) as u32) * 4) as usize;
            assert_eq!(&pixels[at..at + 4], &[80, 100, 200, 255]);
            assert_eq!(
                pixels[((height - 20) * width * 4 + (width - 20) * 4 + 3) as usize],
                0
            );
            surface.end_frame().unwrap();
        }
        // A Shell item can keep its identity while its artwork changes, e.g.
        // the Recycle Bin. Verify replacement reaches the real GPU texture.
        let mut updated = sample_model();
        for color in [[25, 180, 70, 255], [80, 100, 200, 255]] {
            let old = updated.items[0].image.as_ref().unwrap();
            updated.items[0].image = Some(Arc::new(assets::Pixels {
                width: old.width,
                height: old.height,
                data: color.repeat((old.width * old.height) as usize),
            }));
            let target = surface.begin_frame(400, 240).unwrap();
            gpu.paint(&target, 400, 240, 1.0, &updated).unwrap();
            let pixels = surface.readback().unwrap();
            let grid = updated.grid(400.0, 240.0);
            let (x, y) = grid.cell(0, 0);
            let at =
                (((y + 20.0) as u32 * 400 + (x + grid.cell_width / 2.0) as u32) * 4) as usize;
            assert_eq!(&pixels[at..at + 4], &color);
            assert_eq!(gpu.images.len(), 1);
            surface.end_frame().unwrap();
        }
        // An inventory or another pane can still own the pixels after this
        // pane loses the item. Its GPU upload must nevertheless be released.
        let retained_source = updated.items[0].image.clone().unwrap();
        updated.items.clear();
        let target = surface.begin_frame(400, 240).unwrap();
        gpu.paint(&target, 400, 240, 1.0, &updated).unwrap();
        assert!(gpu.images.is_empty());
        assert!(!retained_source.data.is_empty());
        surface.end_frame().unwrap();
        // Alternate the test-only CPU upload path with native drawing to catch lingering buffer
        // references and context state that would make ResizeBuffers fail.
        for (width, height) in [(160, 120), (400, 240)] {
            assert!(surface.present(width, height, &[0; 4]).is_err());
            let pixels = [32, 64, 128, 128].repeat((width * height) as usize);
            surface.present(width, height, &pixels).unwrap();
            let target = surface.begin_frame(width, height).unwrap();
            gpu.paint(&target, width, height, 1.5, &model).unwrap();
            let pixels = surface.readback().unwrap();
            assert_eq!(pixels.len(), (width * height * 4) as usize);
            assert_eq!(
                pixels[((height - 1) * width * 4 + (width - 1) * 4 + 3) as usize],
                0
            );
            surface.end_frame().unwrap();
        }
    }
    unsafe {
        windows_sys::Win32::UI::WindowsAndMessaging::DestroyWindow(window.hwnd().cast());
    }
}
