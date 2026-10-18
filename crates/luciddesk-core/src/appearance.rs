//! Panel materials, themes, and presentation options.

/// Materials that support a strength adjustment in [`Backdrop::Tuned`].
#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, Debug, PartialEq)]
pub enum TunableMaterial {
    Acrylic,
    Mica,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, Debug, PartialEq)]
pub enum Backdrop {
    Tuned {
        material: TunableMaterial,
        strength: u8,
    },
    Translucent {
        opacity: f32,
    },
    Solid {
        color: u32,
        opacity: f32,
    },
    Mica,
    MicaAlt,
    Acrylic,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PanelTheme {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PanelText {
    #[default]
    Auto,
    Light,
    Dark,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct PaneOptions {
    pub corner_radius: f32,
    pub grid_scale: f32,
    pub border: bool,
    pub snap: bool,
    pub text: PanelText,
    pub text_protection: bool,
}

impl PaneOptions {
    pub const GRID_SCALE_RANGE: (f32, f32) = (50.0, 200.0);
    pub const MAX_CORNER_RADIUS: f32 = 24.0;
    pub const DEFAULT: Self = Self {
        corner_radius: 6.0,
        grid_scale: 100.0,
        border: true,
        snap: true,
        text: PanelText::Auto,
        text_protection: false,
    };
}

impl Default for PaneOptions {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl Backdrop {
    pub const DEFAULT: Self = Self::Mica;

    pub const fn solid_default(dark: bool) -> Self {
        Self::Solid {
            color: if dark { 0x202020 } else { 0xf3f3f3 },
            opacity: 0.85,
        }
    }

    #[must_use]
    pub const fn base(self) -> Self {
        match self {
            Self::Tuned { material, .. } => match material {
                TunableMaterial::Acrylic => Self::Acrylic,
                TunableMaterial::Mica => Self::Mica,
            },
            other => other,
        }
    }

    #[must_use]
    pub const fn strength(self) -> Option<u8> {
        match self {
            Self::Tuned { strength, .. } => Some(strength),
            Self::Acrylic | Self::Mica => Some(50),
            _ => None,
        }
    }

    #[must_use]
    pub fn with_strength(self, strength: u8) -> Self {
        let material = match self.base() {
            Self::Acrylic => TunableMaterial::Acrylic,
            Self::Mica => TunableMaterial::Mica,
            _ => return self,
        };
        if strength == 50 {
            self.base()
        } else {
            Self::Tuned {
                material,
                strength: strength.min(100),
            }
        }
    }

    #[must_use]
    pub const fn strength_key(self) -> Option<&'static str> {
        match self.base() {
            Self::Acrylic => Some("material_strength_acrylic"),
            Self::Mica => Some("material_strength_mica"),
            _ => None,
        }
    }

    #[must_use]
    pub const fn translucent() -> Self {
        Self::Translucent { opacity: 0.86 }
    }

    #[must_use]
    pub const fn kind(self) -> BackdropKind {
        match self {
            Self::Tuned { .. } => self.base().kind(),
            Self::Translucent { .. } => BackdropKind::Translucent,
            Self::Solid { .. } => BackdropKind::Solid,
            Self::Mica => BackdropKind::Mica,
            Self::MicaAlt => BackdropKind::MicaAlt,
            Self::Acrylic => BackdropKind::Acrylic,
        }
    }

    #[must_use]
    pub const fn label(self) -> &'static str {
        self.kind().label()
    }
}

impl Default for Backdrop {
    fn default() -> Self {
        Self::DEFAULT
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackdropKind {
    Mica,
    MicaAlt,
    Acrylic,
    Translucent,
    Solid,
}

impl BackdropKind {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Mica => "Mica",
            Self::MicaAlt => "Mica Alt",
            Self::Acrylic => "Desktop Acrylic",
            Self::Translucent => "Translucent",
            Self::Solid => "Solid",
        }
    }
}
