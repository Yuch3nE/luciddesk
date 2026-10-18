//! Device-independent coordinates and pane dimensions.

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PointDip {
    pub x: f32,
    pub y: f32,
}

impl PointDip {
    #[must_use]
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GridPosition {
    pub column: u32,
    pub row: u32,
}

impl GridPosition {
    #[must_use]
    pub const fn new(column: u32, row: u32) -> Self {
        Self { column, row }
    }
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct RectDip {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl RectDip {
    pub const MIN_WIDTH: f32 = 260.0;
    pub const MIN_HEIGHT: f32 = 160.0;

    /// Preserve measured/restored bounds; content-dependent minimums belong to the view.
    #[must_use]
    pub fn from_bounds(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self { x, y, width: width.max(1.0), height: height.max(1.0) }
    }

    #[must_use]
    pub fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width: width.max(Self::MIN_WIDTH),
            height: height.max(Self::MIN_HEIGHT),
        }
    }
}

impl Default for RectDip {
    fn default() -> Self {
        Self::new(120.0, 120.0, 420.0, 360.0)
    }
}
