//! Shared metrics and colors for the canvas controls and native dialogs.
pub const HEIGHT: f32 = 32.0;
pub const TEXT: f32 = 14.0;
pub const RADIUS: f32 = 4.0;
pub const FOCUS_STROKE: f32 = 2.0;
pub const PRIMARY_FOCUS_INSET: f32 = 2.0;
pub const BUTTON_GAP: f32 = 8.0;
pub const BUTTON_PADDING: f32 = 16.0;
pub const ICON_SLOT: f32 = 16.0;
pub const ICON_SIZE: f32 = 12.0;
pub const ICON_GAP: f32 = 8.0;
pub const ROW_INSET: f32 = 16.0;
pub const CHEVRON_UP: &str = "\u{e70e}";
pub const CHEVRON_DOWN: &str = "\u{e70d}";
pub const CHEVRON_RIGHT: &str = "\u{e76c}";
pub const CHECK_MARK: &str = "\u{e73e}";
pub const ARROW_UP: &str = "\u{e74a}";
pub const ARROW_DOWN: &str = "\u{e74b}";
pub const DELETE: &str = "\u{e74d}";
pub const INFO: &str = "\u{e946}";
pub const WARNING: &str = "\u{e7ba}";
pub const ERROR_BADGE: &str = "\u{ea39}";
pub const RADIO_OFF: &str = "\u{ea3a}";
pub const RADIO_ON: &str = "\u{ea3f}";
pub const FOLDER: &str = "\u{e8b7}";
pub const DOCUMENT: &str = "\u{e8a5}";
pub const PHOTO: &str = "\u{e91b}";
pub const BACK_ARROW: &str = "\u{e72b}";
pub const RESET_ICON: &str = "\u{e777}";
pub const EXTERNAL_LINK: &str = "\u{e8a7}";
pub const SEARCH_ICON: &str = "\u{e721}";
pub const CLEAR_ICON: &str = "\u{e711}";
pub const FLUENT_ICON_FONT: &str = "Segoe Fluent Icons";
pub const MDL2_ICON_FONT: &str = "Segoe MDL2 Assets";

/// Fluent 2 vertical elevation border style, independent of renderer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ElevationKind {
    Control,
    Accent,
    TextField,
}

/// Three-stop vertical gradient for a 1px inset Fluent 2 rounded stroke.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ElevationGradient {
    pub top: [f32; 4],
    pub mid: [f32; 4],
    pub bottom: [f32; 4],
    pub stop: f32,
}

/// Colors are stored as 0xRRGGBB, independent of either renderer.
pub struct Palette {
    pub dark: bool,
    pub background: u32,
    pub page_background: u32,
    pub card: u32,
    pub card_border: u32,
    pub flyout: u32,
    pub ink: u32,
    pub muted: u32,
    pub disabled: u32,
    pub disabled_surface: u32,
    pub disabled_border: u32,
    pub border: u32,
    pub hover_border: u32,
    pub accent: u32,
    pub accent_ink: u32,
    pub selection: u32,
    pub on_accent: u32,
    pub button: u32,
    pub button_hover: u32,
    pub primary_hover: u32,
    pub scroll_thumb: u32,
    pub success: u32,
    pub danger: u32,
    pub info: u32,
    pub toggle_off_border: u32,
    pub toggle_off_border_hover: u32,
    pub toggle_off_thumb: u32,
    pub segment_active: u32,
    pub slider_thumb: u32,
}

impl Palette {
    pub const fn for_theme(dark: bool) -> Self {
        if dark {
            Self {
                dark: true,
                background: 0x202020,
                page_background: 0x242424,
                card: 0x2c2c2c,
                card_border: 0x373737,
                flyout: 0x2c2c2c,
                ink: 0xf7f7f7,
                muted: 0xbababa,
                disabled: 0x808080,
                disabled_surface: 0x2e2e2e,
                disabled_border: 0x404040,
                border: 0x4a4a4a,
                hover_border: 0x606060,
                accent: 0x4fb3f6,
                accent_ink: 0x7ccbff,
                selection: 0x343434,
                on_accent: 0x0b141c,
                button: 0x363636,
                button_hover: 0x3f3f3f,
                primary_hover: 0x62bdf8,
                scroll_thumb: 0x9c9c9c,
                success: 0x6ccb5f,
                danger: 0xf4868e,
                info: 0x7ccbff,
                toggle_off_border: 0x8c8c8c,
                toggle_off_border_hover: 0xb2b2b2,
                toggle_off_thumb: 0xcacaca,
                segment_active: 0x3e3e3e,
                slider_thumb: 0x4a4a4a,
            }
        } else {
            Self {
                dark: false,
                background: 0xf2f2f2,
                page_background: 0xf7f7f7,
                card: 0xffffff,
                card_border: 0xe4e4e4,
                flyout: 0xfcfcfc,
                ink: 0x1a1a1a,
                muted: 0x5d5d5d,
                disabled: 0x969696,
                disabled_surface: 0xf3f3f3,
                disabled_border: 0xe2e2e2,
                border: 0xd4d4d4,
                hover_border: 0xb8b8b8,
                accent: 0x005fb8,
                accent_ink: 0x004c99,
                selection: 0xeaeaea,
                on_accent: 0xffffff,
                button: 0xfcfcfc,
                button_hover: 0xf5f5f5,
                primary_hover: 0x146bc0,
                scroll_thumb: 0x888888,
                success: 0x0f7b0f,
                danger: 0xc42b1c,
                info: 0x005fb8,
                toggle_off_border: 0x7e7e7e,
                toggle_off_border_hover: 0x5c5c5c,
                toggle_off_thumb: 0x5e5e5e,
                segment_active: 0xffffff,
                slider_thumb: 0xffffff,
            }
        }
    }

    pub const fn unpack_rgb(rgb: u32) -> [f32; 3] {
        [
            ((rgb >> 16) & 255) as f32 / 255.0,
            ((rgb >> 8) & 255) as f32 / 255.0,
            (rgb & 255) as f32 / 255.0,
        ]
    }

    pub const fn rgba(rgb: u32, alpha: f32) -> [f32; 4] {
        let [r, g, b] = Self::unpack_rgb(rgb);
        [r, g, b, alpha]
    }

    pub fn neutral(&self, alpha: f32) -> [f32; 4] {
        let channel = if self.dark { 1.0 } else { 0.0 };
        [channel, channel, channel, alpha]
    }

    pub fn divider(&self) -> [f32; 4] {
        self.neutral(if self.dark { 0.072 } else { 0.065 })
    }

    pub fn page_border(&self) -> [f32; 4] {
        self.neutral(if self.dark { 0.065 } else { 0.075 })
    }

    pub fn specimen_fill(&self) -> [f32; 4] {
        self.neutral(if self.dark { 0.036 } else { 0.032 })
    }

    pub fn card_border_rgba(&self, native: bool) -> [f32; 4] {
        if native {
            self.neutral(if self.dark { 0.068 } else { 0.075 })
        } else {
            Self::rgba(self.card_border, 1.0)
        }
    }

    pub fn flyout_border(&self) -> [f32; 4] {
        self.neutral(if self.dark { 0.09 } else { 0.08 })
    }

    pub fn flyout_hover(&self, progress: f32) -> [f32; 4] {
        self.neutral((if self.dark { 0.075 } else { 0.045 }) * progress.clamp(0.0, 1.0))
    }

    pub fn subtle_hover(&self, progress: f32) -> [f32; 4] {
        self.neutral((if self.dark { 0.05 } else { 0.032 }) * progress.clamp(0.0, 1.0))
    }

    pub fn subtle_selection(&self, progress: f32) -> [f32; 4] {
        self.neutral((if self.dark { 0.07 } else { 0.045 }) * progress.clamp(0.0, 1.0))
    }

    pub fn elevation_gradient(
        &self,
        kind: ElevationKind,
        hover: f32,
        press: f32,
    ) -> ElevationGradient {
        let hover = hover.clamp(0.0, 1.0);
        let press = press.clamp(0.0, 1.0);
        let relief = (1.0 - press).clamp(0.0, 1.0);
        match kind {
            ElevationKind::Accent if self.dark => ElevationGradient {
                top: [1.0, 1.0, 1.0, 0.10 * relief + 0.04 * press],
                mid: [1.0, 1.0, 1.0, 0.06 * relief + 0.04 * press],
                bottom: [0.0, 0.0, 0.0, 0.18 * relief],
                stop: 0.86,
            },
            ElevationKind::Accent => {
                let top = [1.0, 1.0, 1.0, 0.08 * relief];
                ElevationGradient {
                    top,
                    mid: top,
                    bottom: [0.0, 0.0, 0.0, 0.36 * relief + 0.08 * press],
                    stop: 0.86,
                }
            }
            ElevationKind::TextField => {
                let side = self.neutral(if self.dark { 0.08 } else { 0.06 });
                let bottom = self.neutral(if self.dark { 0.44 } else { 0.40 });
                ElevationGradient {
                    top: side,
                    mid: side,
                    bottom,
                    stop: 0.90,
                }
            }
            ElevationKind::Control if self.dark => {
                let rest = 0.082 + 0.028 * hover * relief;
                let highlight = rest + 0.040 * relief;
                let side = [1.0, 1.0, 1.0, rest];
                ElevationGradient {
                    top: [1.0, 1.0, 1.0, highlight],
                    mid: side,
                    bottom: side,
                    stop: 0.16,
                }
            }
            ElevationKind::Control => {
                let rest = 0.088 + 0.028 * hover * relief;
                let shadow = rest + 0.105 * relief;
                let side = [0.0, 0.0, 0.0, rest];
                ElevationGradient {
                    top: side,
                    mid: side,
                    bottom: [0.0, 0.0, 0.0, shadow],
                    stop: 0.84,
                }
            }
        }
    }
}

pub const fn colorref(rgb: u32) -> u32 {
    ((rgb & 0xff) << 16) | (rgb & 0xff00) | (rgb >> 16)
}
