//! Encoding and validation of persisted appearance preferences.
use crate::StoreError;
use luciddesk_core::{Backdrop, Workspace};

pub(super) fn encode_panel_text(text: luciddesk_core::PanelText) -> &'static str {
    match text {
        luciddesk_core::PanelText::Auto => "auto",
        luciddesk_core::PanelText::Light => "light",
        luciddesk_core::PanelText::Dark => "dark",
    }
}

pub(super) fn workspace_preferences(
    workspace: &Workspace,
) -> Result<Vec<(&'static str, String)>, StoreError> {
    let options = workspace.pane_options();
    let mut values = vec![(
        "pane_options",
        format!(
            "{}|{}|{}|{}|{}|{}",
            options.corner_radius,
            options.border,
            options.snap,
            encode_panel_text(options.text),
            options.text_protection, options.grid_scale
        ),
    )];
    if let Some((theme, backdrop)) = workspace.appearance() {
        let theme = match theme {
            luciddesk_core::PanelTheme::System => "system",
            luciddesk_core::PanelTheme::Light => "light",
            luciddesk_core::PanelTheme::Dark => "dark",
        };
        let (kind, opacity, color) = encode_backdrop(backdrop);
        decode_backdrop(kind, opacity, color)?;
        if let (Some(key), Some(strength)) = (backdrop.strength_key(), backdrop.strength()) {
            values.push((key, strength.to_string()));
        }
        if let Backdrop::Solid { color, opacity } = backdrop {
            values.push(("solid_style", format!("{color}|{opacity}")));
        }
        values.push((
            "appearance",
            format!(
                "{theme}|{kind}|{}|{}",
                opacity.unwrap_or(1.0),
                color.map(|v| v.to_string()).unwrap_or_default()
            ),
        ));
    }
    Ok(values)
}

pub(super) fn equivalent_backdrop(a: Backdrop, b: Backdrop) -> bool {
    let normalize = |v: Backdrop| v.strength().map_or(v, |strength| v.with_strength(strength));
    normalize(a) == normalize(b)
}

pub(super) fn encode_backdrop(backdrop: Backdrop) -> (&'static str, Option<f32>, Option<u32>) {
    match backdrop {
        Backdrop::Tuned { material, strength } => (
            match material {
                luciddesk_core::TunableMaterial::Acrylic => "acrylic_tuned",
                luciddesk_core::TunableMaterial::Mica => "mica_tuned",
            },
            Some(f32::from(strength) / 100.0),
            None,
        ),
        Backdrop::Mica => ("mica", None, None),
        Backdrop::MicaAlt => ("mica_alt", None, None),
        Backdrop::Acrylic => ("acrylic", None, None),
        Backdrop::Translucent { opacity } => ("translucent", Some(opacity), None),
        Backdrop::Solid { color, opacity } => ("solid", Some(opacity), Some(color)),
    }
}

pub(super) fn decode_backdrop(
    kind: &str,
    opacity: Option<f32>,
    color: Option<u32>,
) -> Result<Backdrop, StoreError> {
    let opacity = opacity.unwrap_or(0.86);
    if !opacity.is_finite() || !(0.0..=1.0).contains(&opacity) {
        return Err(StoreError::InvalidData("invalid backdrop opacity".into()));
    }
    Ok(match kind {
        "acrylic_tuned" => Backdrop::Acrylic.with_strength((opacity * 100.0).round() as u8),
        "mica_tuned" => Backdrop::Mica.with_strength((opacity * 100.0).round() as u8),
        "mica" => Backdrop::Mica,
        "mica_alt" => Backdrop::MicaAlt,
        "acrylic" => Backdrop::Acrylic,
        "translucent" => Backdrop::Translucent { opacity },
        "solid" => {
            let color = color
                .filter(|v| *v <= 0xffffff)
                .ok_or_else(|| StoreError::InvalidData("invalid solid color".into()))?;
            Backdrop::Solid { color, opacity }
        }
        _ => return Err(StoreError::InvalidData(format!("unknown backdrop {kind}"))),
    })
}
