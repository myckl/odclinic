//! Shared visual language for the desktop shell.
//!
//! "Clinic daylight": quiet paper and slate surfaces, one deep teal accent for
//! the current context, and enough density for a receptionist to scan a day
//! without losing the meaning of a colour. No per-screen theme logic; every
//! module inherits these tokens.

use egui::{
    Color32, CornerRadius, FontFamily, FontId, Margin, Stroke, TextStyle, Vec2,
    style::{Selection, WidgetVisuals},
};

fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

pub fn ink() -> Color32 {
    rgb(23, 46, 54)
}

pub fn muted() -> Color32 {
    rgb(93, 115, 122)
}

pub fn paper() -> Color32 {
    rgb(247, 249, 249)
}

pub fn panel() -> Color32 {
    rgb(237, 242, 243)
}

pub fn line() -> Color32 {
    rgb(203, 214, 216)
}

pub fn teal() -> Color32 {
    rgb(15, 111, 108)
}

pub fn success() -> Color32 {
    rgb(47, 109, 83)
}

pub fn danger() -> Color32 {
    rgb(163, 48, 43)
}

pub fn warning() -> Color32 {
    rgb(138, 90, 18)
}

pub fn info() -> Color32 {
    rgb(48, 92, 145)
}

/// Quiet, distinguishable hues for provider lanes on the scheduling board.
pub fn provider_palette() -> [Color32; 6] {
    [
        rgb(15, 111, 108),
        rgb(48, 92, 145),
        rgb(112, 84, 145),
        rgb(160, 106, 40),
        rgb(47, 109, 83),
        rgb(151, 76, 86),
    ]
}

/// Soft blockout colours, still legible when text sits on top of them.
pub fn hold_palette() -> [Color32; 6] {
    [
        rgb(236, 202, 120),
        rgb(176, 201, 227),
        rgb(183, 215, 188),
        rgb(224, 187, 197),
        rgb(172, 211, 205),
        rgb(216, 221, 222),
    ]
}

/// Light status fills for the legacy grid and board block outlines.
pub fn status_fill(status: od_domain::AppointmentStatus) -> Color32 {
    use od_domain::AppointmentStatus as S;
    match status {
        S::Scheduled => rgb(221, 234, 240),
        S::Complete => rgb(220, 235, 225),
        S::Broken => rgb(244, 222, 220),
        S::Planned => rgb(234, 237, 237),
        S::Unscheduled => rgb(245, 233, 209),
        S::UnschedList => rgb(237, 234, 224),
        S::Asap => rgb(248, 224, 199),
        S::PtNote => rgb(246, 239, 199),
        S::PtNoteCompleted => rgb(223, 235, 219),
    }
}

/// Apply the design tokens to every egui style (light and dark).
pub fn apply(ctx: &egui::Context) {
    ctx.all_styles_mut(|style| {
        style.text_styles = [
            (
                TextStyle::Small,
                FontId::new(11.0, FontFamily::Proportional),
            ),
            (TextStyle::Body, FontId::new(13.0, FontFamily::Proportional)),
            (
                TextStyle::Button,
                FontId::new(13.0, FontFamily::Proportional),
            ),
            (
                TextStyle::Monospace,
                FontId::new(12.0, FontFamily::Monospace),
            ),
            (
                TextStyle::Heading,
                FontId::new(19.0, FontFamily::Proportional),
            ),
        ]
        .into();

        style.spacing.item_spacing = Vec2::new(8.0, 6.0);
        style.spacing.window_margin = Margin::same(12);
        style.spacing.button_padding = Vec2::new(10.0, 5.0);
        style.spacing.interact_size = Vec2::new(52.0, 24.0);
        style.spacing.extra_text_line_spacing = 2.0;

        let visuals = &mut style.visuals;
        visuals.dark_mode = false;
        visuals.override_text_color = Some(ink());
        visuals.weak_text_color = Some(muted());
        visuals.hyperlink_color = teal();
        visuals.faint_bg_color = rgb(239, 244, 244);
        visuals.extreme_bg_color = rgb(255, 255, 255);
        visuals.text_edit_bg_color = Some(rgb(251, 253, 253));
        visuals.code_bg_color = rgb(234, 240, 241);
        visuals.warn_fg_color = rgb(138, 90, 18);
        visuals.error_fg_color = rgb(163, 48, 43);
        visuals.panel_fill = panel();
        visuals.window_fill = paper();
        visuals.window_stroke = Stroke::new(1.0, line());
        visuals.window_corner_radius = CornerRadius::same(8);
        visuals.menu_corner_radius = CornerRadius::same(6);

        let stroke = |color| Stroke::new(1.0, color);
        visuals.widgets.noninteractive = WidgetVisuals {
            bg_fill: panel(),
            weak_bg_fill: panel(),
            bg_stroke: stroke(line()),
            corner_radius: CornerRadius::same(4),
            fg_stroke: stroke(ink()),
            expansion: 0.0,
        };
        visuals.widgets.inactive = WidgetVisuals {
            bg_fill: rgb(244, 248, 248),
            weak_bg_fill: rgb(244, 248, 248),
            bg_stroke: stroke(line()),
            corner_radius: CornerRadius::same(4),
            fg_stroke: stroke(ink()),
            expansion: 0.0,
        };
        visuals.widgets.hovered = WidgetVisuals {
            bg_fill: rgb(224, 240, 237),
            weak_bg_fill: rgb(224, 240, 237),
            bg_stroke: stroke(teal()),
            corner_radius: CornerRadius::same(4),
            fg_stroke: stroke(ink()),
            expansion: 0.0,
        };
        visuals.widgets.active = WidgetVisuals {
            bg_fill: rgb(205, 227, 223),
            weak_bg_fill: rgb(205, 227, 223),
            bg_stroke: stroke(teal()),
            corner_radius: CornerRadius::same(4),
            fg_stroke: stroke(ink()),
            expansion: 0.0,
        };
        visuals.widgets.open = visuals.widgets.hovered;
        visuals.selection = Selection {
            bg_fill: rgb(211, 231, 228),
            stroke: stroke(teal()),
        };
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_light_and_legible() {
        assert!(ink().r() < 60 && ink().g() < 80);
        assert!(teal().r() < 40 && teal().g() > 90);
        assert!(paper().r() > 240);
        assert!(success().g() > success().r());
    }
}
