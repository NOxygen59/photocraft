//! Phones (iPhone, and an iPad app narrowed by Split View): a compact layout when the window's
//! shorter side has no room for the desktop one. The shell stays the same; what doesn't fit
//! changes:
//!
//! - the safe area (the notch or Dynamic Island, the rounded corners, the home indicator) is left
//!   to the chrome colour, so nothing sits under them;
//! - the menus are one button holding them as submenus (`menus::menu_button_compact`);
//! - the dock is its icon rail: a panel group opens as a sheet over the canvas, beside the rail.

use egui::{Align2, Color32, Frame, Id, Margin, Rect, Stroke, Ui, vec2};

use crate::dock::Group;
use crate::theme::Tokens;
use crate::PhotocraftApp;

/// Below this shorter side (points) the window is a phone's: every iPhone, in either
/// orientation, and no full-screen iPad.
const COMPACT_SIDE: f32 = 600.0;

/// Is the window laid out for a phone? iOS only (a desktop window made this small keeps the
/// desktop layout), unless `PHOTOCRAFT_PHONE_LAYOUT` previews it ([`preview`]).
pub fn is_compact(ctx: &egui::Context) -> bool {
    (cfg!(target_os = "ios") || preview().is_some()) && ctx.input(|i| i.content_rect()).size().min_elem() < COMPACT_SIDE
}

/// `PHOTOCRAFT_PHONE_LAYOUT=top,left,bottom,right` previews the phone layout off iOS (the snapshot
/// example, a desktop window made phone-sized) with those safe area insets, in points: e.g.
/// `59,0,34,0` for a recent iPhone in portrait.
fn preview() -> Option<[f32; 4]> {
    static PREVIEW: std::sync::OnceLock<Option<[f32; 4]>> = std::sync::OnceLock::new();
    *PREVIEW.get_or_init(|| {
        let text = std::env::var("PHOTOCRAFT_PHONE_LAYOUT").ok()?;
        let v: Vec<f32> = text.split(',').filter_map(|p| p.trim().parse().ok()).collect();
        Some(match v[..] {
            [t, l, b, r] => [t, l, b, r],
            _ => [0.0; 4],
        })
    })
}

/// Keeps the safe area clear (compact layout only): a strip of chrome colour along each edge the
/// system draws over. First of the panels, so it is the outermost.
pub fn safe_area(app: &PhotocraftApp, ui: &mut Ui) {
    if !is_compact(ui.ctx()) {
        return;
    }
    let Some(insets) = app.services.safe_area.map(|f| f()).or_else(preview) else { return };
    let [top, left, bottom, right] = insets.map(|v| if v.is_finite() { v.clamp(0.0, 200.0) } else { 0.0 });
    let fill = Tokens::get(ui.ctx()).chrome;
    let strip = |size: f32| (size > 0.5).then_some(size);
    if let Some(s) = strip(top) {
        egui::Panel::top("safe-area-top").exact_size(s).frame(Frame::NONE.fill(fill)).show(ui, |_| {});
    }
    if let Some(s) = strip(bottom) {
        egui::Panel::bottom("safe-area-bottom").exact_size(s).frame(Frame::NONE.fill(fill)).show(ui, |_| {});
    }
    if let Some(s) = strip(left) {
        egui::Panel::left("safe-area-left").exact_size(s).resizable(false).frame(Frame::NONE.fill(fill)).show(ui, |_| {});
    }
    if let Some(s) = strip(right) {
        egui::Panel::right("safe-area-right").exact_size(s).resizable(false).frame(Frame::NONE.fill(fill)).show(ui, |_| {});
    }
}

fn sheet_id() -> Id {
    Id::new("phone-dock-sheet")
}

/// The panel group open as a sheet, if any.
pub fn sheet(ctx: &egui::Context) -> Option<Group> {
    ctx.data(|d| d.get_temp::<Option<Group>>(sheet_id())).flatten()
}

/// The rail's button for `group`: opens its sheet, or closes it when it is the one open.
pub fn toggle_sheet(ctx: &egui::Context, group: Group) {
    let next = (sheet(ctx) != Some(group)).then_some(group);
    ctx.data_mut(|d| d.insert_temp(sheet_id(), next));
}

/// The open group's sheet, over the canvas beside the rail: `room` is what the panels left free.
pub fn show_sheet(app: &mut PhotocraftApp, ui: &mut Ui, room: Rect, body: fn(&mut PhotocraftApp, &mut Ui, Group, usize)) {
    let ctx = ui.ctx().clone();
    let Some(group) = sheet(&ctx) else { return };
    let t = Tokens::get(&ctx);
    let width = (room.width() - 24.0).clamp(160.0, 340.0);
    let rect = Rect::from_min_size(room.right_top() - vec2(width, 0.0), vec2(width, room.height()));
    egui::Area::new(sheet_id().with("area"))
        .order(egui::Order::Foreground)
        .pivot(Align2::LEFT_TOP)
        .fixed_pos(rect.min)
        .constrain(false)
        .show(&ctx, |ui| {
            Frame::NONE
                .fill(t.dock)
                .stroke(Stroke::new(1.0, t.separator))
                .shadow(egui::Shadow { offset: [-4, 0], blur: 16, spread: 0, color: Color32::from_black_alpha(90) })
                .inner_margin(Margin::same(6))
                .show(ui, |ui| {
                    ui.set_width(rect.width() - 12.0);
                    ui.set_height(rect.height() - 12.0);
                    ui.horizontal(|ui| {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if crate::icons::button(ui, "x", 28.0, false, crate::i18n::t("Close")).clicked() {
                                ctx.data_mut(|d| d.insert_temp::<Option<Group>>(sheet_id(), None));
                            }
                        });
                    });
                    crate::dock::show(app, ui, &[group], body);
                });
        });
}
