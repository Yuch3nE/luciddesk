//! Shared metrics and colors for the canvas controls and native dialogs.
pub const HEIGHT: f32 = 32.0;
pub const TEXT: f32 = 14.0;
pub const RADIUS: f32 = 6.0;
pub const FOCUS_STROKE: f32 = 2.0;
pub const PRIMARY_FOCUS_INSET: f32 = 3.0;
pub const BUTTON_GAP: f32 = 8.0;
pub const BUTTON_PADDING: f32 = 16.0;
pub const ICON_SLOT: f32 = 16.0;
pub const ICON_SIZE: f32 = 12.0;
pub const ICON_GAP: f32 = 8.0;
pub const CHEVRON_UP: &str = "\u{e70e}";
pub const CHEVRON_DOWN: &str = "\u{e70d}";
pub const CHEVRON_RIGHT: &str = "\u{e76c}";
pub const CHECK_MARK: &str = "\u{e73e}";
pub const ARROW_UP: &str = "\u{e74a}";
pub const ARROW_DOWN: &str = "\u{e74b}";
pub const INFO: &str = "\u{e946}";
pub const WARNING: &str = "\u{e7ba}";
pub const ERROR_BADGE: &str = "\u{ea39}";
pub const RADIO_OFF: &str = "\u{ea3a}";
pub const RADIO_ON: &str = "\u{ea3f}";
pub const FOLDER: &str = "\u{e8b7}";
pub const DOCUMENT: &str = "\u{e8a5}";
pub const PHOTO: &str = "\u{e91b}";
pub const FLUENT_ICON_FONT: &str = "Segoe Fluent Icons";
pub const MDL2_ICON_FONT: &str = "Segoe MDL2 Assets";

/// Colors are stored as 0xRRGGBB, independent of either renderer.
pub struct Palette {
    pub background: u32,
    pub page_background: u32,
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
}

impl Palette {
    pub const fn for_theme(dark: bool) -> Self {
        if dark {
            Self {
                background: 0x202020,
                page_background: 0x242424,
                ink: 0xf5f5f5,
                muted: 0xadadad,
                disabled: 0x8b9097,
                disabled_surface: 0x292b2d,
                disabled_border: 0x3c4045,
                border: 0x51565d,
                hover_border: 0x747f8a,
                accent: 0x76b9ed,
                accent_ink: 0xa0cff3,
                selection: 0x293d4d,
                on_accent: 0x202020,
                button: 0x2d2d2d,
                button_hover: 0x373c42,
                primary_hover: 0x91caf4,
                scroll_thumb: 0xadadad,
            }
        } else {
            Self {
                background: 0xe9edf2,
                page_background: 0xf0f3f7,
                ink: 0x202833,
                muted: 0x566170,
                disabled: 0x75808d,
                disabled_surface: 0xf1f4f7,
                disabled_border: 0xd4dce5,
                border: 0xb7c4d2,
                hover_border: 0x839bb3,
                accent: 0x0969b8,
                accent_ink: 0x175c97,
                selection: 0xe5eff9,
                on_accent: 0xffffff,
                button: 0xffffff,
                button_hover: 0xe9eff6,
                primary_hover: 0x0b5694,
                scroll_thumb: 0x758293,
            }
        }
    }
}

pub const fn colorref(rgb: u32) -> u32 {
    ((rgb & 0xff) << 16) | (rgb & 0xff00) | (rgb >> 16)
}
