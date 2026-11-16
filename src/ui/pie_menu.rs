//! MorningDAW-inspired Smart Radial Pie Menu & Command Palette (`Ctrl+K`) for DawnStudio.
//!
//! Supports dual interaction modes on both Android touchscreens and Desktop mice:
//! - **Hold-and-Flick (Gesture Mode)**: Hold a trigger (long-press, right-drag, middle-click, or the Quick Pie button),
//!   drag outward into a radial wedge (`r >= inner_r`), and release to immediately execute that slice's action.
//! - **Tap / Click Mode**: Tap to open the radial ring (releasing inside `inner_r`), then tap any wedge to execute,
//!   or tap the center hub / outside / press `Escape` to dismiss.

use std::f32::consts::{FRAC_PI_2, TAU};

use eframe::egui::{
    self, Align2, Color32, Id, Key, LayerId, Mesh, Order, Painter, PointerButton, Pos2, Rect,
    Sense, Shape, Stroke, StrokeKind, TextEdit, Vec2, epaint::Vertex, pos2, vec2,
};

use crate::icons::{IconId, draw_icon};
use crate::metrics::{FontSize, Metrics};
use crate::song::{ChannelKind, SynthKind};
use crate::theme::{DISABLED_ALPHA, DAWN_GOLD_JSON, MORNING_EMBER_JSON, WYVERN_CYAN_JSON};
use crate::ui::UiCtx;
use crate::ui::add_channel_menu::AddChannelPick;
use crate::ui::track_menu::TrackOp;
use crate::ui::views::{MenuTab, Nav, Overlay, View};

/// Inner hub radius in U: inside this circle is the neutral / cancel zone.
pub const INNER_R_U: f32 = 1.35;
/// Outer ring radius in U: wedges span `INNER_R_U .. OUTER_R_U`.
pub const OUTER_R_U: f32 = 4.25;
/// Mid-radius where wedge icons and labels sit, in U.
pub const LABEL_R_U: f32 = 2.82;
/// Margin from screen edges when clamping the pie center, in U.
const EDGE_MARGIN_U: f32 = 0.35;
/// Number of arc subdivisions per wedge for smooth GPU circle rendering.
const ARC_STEPS: usize = 16;

/// One slice (wedge) of a radial pie menu.
#[derive(Clone, Debug)]
pub struct PieSlice<T> {
    pub id: T,
    pub label: String,
    pub shortcut: &'static str,
    pub icon: Option<IconId>,
    pub enabled: bool,
}

impl<T> PieSlice<T> {
    pub fn new(id: T, label: impl Into<String>, icon: Option<IconId>) -> Self {
        Self {
            id,
            label: label.into(),
            shortcut: "",
            icon,
            enabled: true,
        }
    }

    pub fn shortcut(mut self, shortcut: &'static str) -> Self {
        self.shortcut = shortcut;
        self
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }
}

/// Output of [`show`] for one frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PieResult<T> {
    pub selected: Option<T>,
    pub close: bool,
}

#[derive(Clone, Copy, Debug, Default)]
struct PieAnimState {
    opened_at: Option<f64>,
    /// True once the initial press that opened the pie has been released inside the hub,
    /// switching the menu from hold-and-flick mode into persistent click/tap mode.
    settled_click_mode: bool,
}

/// Clamps `anchor` so the entire outer ring of radius `OUTER_R_U` stays within `bounds`.
pub fn clamped_center(anchor: Pos2, bounds: Rect, m: &Metrics) -> Pos2 {
    let pad = m.u(OUTER_R_U + EDGE_MARGIN_U);
    let min_x = (bounds.left() + pad).min(bounds.center().x);
    let max_x = (bounds.right() - pad).max(bounds.center().x);
    let min_y = (bounds.top() + pad).min(bounds.center().y);
    let max_y = (bounds.bottom() - pad).max(bounds.center().y);
    pos2(anchor.x.clamp(min_x, max_x), anchor.y.clamp(min_y, max_y))
}

/// Center angle (in radians, `-FRAC_PI_2` = North/Top, clockwise) of slice `index` out of `count`.
pub fn slice_angle(index: usize, count: usize) -> f32 {
    let step = TAU / count.max(1) as f32;
    -FRAC_PI_2 + index as f32 * step
}

/// Center point of wedge `index` (out of `count`) in screen coordinates, for layout and unit tests.
pub fn wedge_center(anchor: Pos2, bounds: Rect, count: usize, index: usize, m: &Metrics) -> Pos2 {
    let c = clamped_center(anchor, bounds, m);
    let angle = slice_angle(index, count);
    let r = m.u(LABEL_R_U);
    pos2(c.x + r * angle.cos(), c.y + r * angle.sin())
}

/// Determines which slice `pos` points to relative to `center`, if `pos` is within `inner_r ..= outer_r * 1.35`.
pub fn hovered_slice(
    center: Pos2,
    pos: Pos2,
    count: usize,
    inner_r: f32,
    outer_r: f32,
) -> Option<usize> {
    if count == 0 {
        return None;
    }
    let d = pos - center;
    let dist = d.length();
    if dist < inner_r || dist > outer_r * 1.35 {
        return None;
    }
    let step = TAU / count as f32;
    // Angle relative to top (-PI/2), shifted by half a step so slice 0 is centered at top.
    let raw = d.y.atan2(d.x) - (-FRAC_PI_2 - step * 0.5);
    let wrapped = raw.rem_euclid(TAU);
    let idx = (wrapped / step).floor() as usize;
    Some(idx.min(count - 1))
}

/// Renders an interactive MorningDAW-style Radial Pie Menu at `anchor` within `bounds`.
pub fn show<T: Copy>(
    ctx: &egui::Context,
    id: Id,
    anchor: Pos2,
    bounds: Rect,
    slices: &[PieSlice<T>],
    cx: &UiCtx,
) -> PieResult<T> {
    if slices.is_empty() {
        return PieResult {
            selected: None,
            close: true,
        };
    }

    let (c, m) = (&cx.theme.colors, cx.m);
    let now = ctx.input(|i| i.time);
    let state_id = id.with("pie_anim");
    let mut state = ctx.data_mut(|d| d.get_temp::<PieAnimState>(state_id).unwrap_or_default());
    let opened_at = *state.opened_at.get_or_insert(now);
    let elapsed = (now - opened_at).max(0.0) as f32;
    // Fast 110ms pop-in animation (0.82 -> 1.0).
    let t = (elapsed / 0.11).clamp(0.0, 1.0);
    let scale = 0.82 + 0.18 * (1.0 - (1.0 - t).powi(3));
    if t < 1.0 {
        ctx.request_repaint();
    }

    let center = clamped_center(anchor, bounds, m);
    let inner_r = m.u(INNER_R_U) * scale;
    let outer_r = m.u(OUTER_R_U) * scale;
    let label_r = m.u(LABEL_R_U) * scale;
    let count = slices.len();
    let step = TAU / count as f32;

    let mut selected = None;
    let mut close = false;

    egui::Area::new(id)
        .order(Order::Foreground)
        .fixed_pos(bounds.min)
        .constrain(false)
        .fade_in(false)
        .show(ctx, |ui| {
            let backdrop = ui.allocate_rect(bounds, Sense::click_and_drag());
            let painter = ctx.layer_painter(LayerId::new(Order::Foreground, id.with("paint")));

            // Subtle dark backdrop vignette around the pie menu.
            painter.circle_filled(
                center,
                outer_r * 1.12,
                Color32::from_black_alpha(96),
            );

            let pointer_pos = ui.input(|i| i.pointer.interact_pos().or(i.pointer.latest_pos()));
            let active_idx = pointer_pos
                .and_then(|p| hovered_slice(center, p, count, inner_r, outer_r));

            let primary_released = ui.input(|i| i.pointer.primary_released());
            let secondary_released = ui.input(|i| i.pointer.button_released(PointerButton::Secondary));
            let middle_released = ui.input(|i| i.pointer.button_released(PointerButton::Middle));
            let any_released = primary_released || secondary_released || middle_released;
            let primary_clicked = backdrop.clicked();

            // Dual-mode gesture resolution:
            // 1. If the user releases any button while aiming at a valid wedge, select it immediately (Flick or Click).
            // 2. If the user releases inside the inner hub on the opening gesture, latch into `settled_click_mode`.
            // 3. Once in `settled_click_mode`, clicking the inner hub or outside `outer_r` closes the menu.
            if any_released {
                if let Some(idx) = active_idx {
                    if slices[idx].enabled {
                        selected = Some(slices[idx].id);
                        close = true;
                    }
                } else if let Some(p) = pointer_pos {
                    let dist = (p - center).length();
                    if dist < inner_r {
                        if state.settled_click_mode && primary_clicked {
                            close = true;
                        } else {
                            state.settled_click_mode = true;
                        }
                    } else if dist > outer_r * 1.05 {
                        close = true;
                    }
                }
            }

            if ui.input(|i| i.key_pressed(Key::Escape)) {
                close = true;
            }

            // Paint each radial wedge.
            let base_fill = c.menu_fill.to_color32().gamma_multiply(0.95);
            let hover_fill = c.accent.to_color32().gamma_multiply(0.32);
            let border_col = c.background.to_color32();
            let accent_col = c.accent.to_color32();
            let text_col = c.text.to_color32();

            for (i, slice) in slices.iter().enumerate() {
                let is_hovered = active_idx == Some(i) && slice.enabled;
                let a0 = slice_angle(i, count) - step * 0.5;
                let a1 = a0 + step;
                let fill = if is_hovered { hover_fill } else { base_fill };
                paint_annular_wedge(&painter, center, inner_r, outer_r, a0, a1, fill);

                // Outer rim highlight on hovered slice, subtle rim on resting slices.
                let rim_stroke = if is_hovered {
                    Stroke::new(3.0 / m.ppp(), accent_col)
                } else {
                    Stroke::new(1.2 / m.ppp(), text_col.gamma_multiply(0.18))
                };
                paint_arc(&painter, center, outer_r, a0, a1, rim_stroke);

                // Radial spoke divider line.
                let p_in = pos2(center.x + inner_r * a0.cos(), center.y + inner_r * a0.sin());
                let p_out = pos2(center.x + outer_r * a0.cos(), center.y + outer_r * a0.sin());
                painter.line_segment(
                    [p_in, p_out],
                    Stroke::new(1.5 / m.ppp(), border_col),
                );

                // Icon + label inside the wedge.
                let mid = slice_angle(i, count);
                let item_pos = pos2(
                    center.x + label_r * mid.cos(),
                    center.y + label_r * mid.sin(),
                );
                let alpha = if slice.enabled { 1.0 } else { DISABLED_ALPHA };
                let fg = if is_hovered {
                    accent_col
                } else {
                    text_col.gamma_multiply(alpha)
                };

                if let Some(icon) = slice.icon {
                    let icon_center = pos2(item_pos.x, item_pos.y - m.u(0.32));
                    let icon_rect = Rect::from_center_size(icon_center, Vec2::splat(m.u(0.95)));
                    draw_icon(&painter, icon_rect, icon, fg, 0.65, m);
                    painter.text(
                        pos2(item_pos.x, item_pos.y + m.u(0.34)),
                        Align2::CENTER_CENTER,
                        &slice.label,
                        m.font_id(FontSize::Pt8),
                        fg,
                    );
                } else {
                    painter.text(
                        item_pos,
                        Align2::CENTER_CENTER,
                        &slice.label,
                        m.font_id(FontSize::Pt10),
                        fg,
                    );
                }

                if !slice.shortcut.is_empty() {
                    painter.text(
                        pos2(item_pos.x, item_pos.y + m.u(0.68)),
                        Align2::CENTER_CENTER,
                        slice.shortcut,
                        m.font_id(FontSize::Pt6),
                        fg.gamma_multiply(0.65),
                    );
                }
            }

            // Center hub circle.
            painter.circle_filled(center, inner_r, c.control_fill.to_color32());
            painter.circle_stroke(
                center,
                inner_r,
                Stroke::new(2.0 / m.ppp(), accent_col.gamma_multiply(0.75)),
            );

            // Directional aiming line from hub center toward active slice.
            if let (Some(idx), Some(p)) = (active_idx, pointer_pos) {
                let dir = (p - center).normalized();
                let tip = center + dir * (inner_r * 0.88);
                painter.line_segment(
                    [center, tip],
                    Stroke::new(2.0 / m.ppp(), accent_col),
                );
                painter.circle_filled(tip, m.u(0.12), accent_col);
                painter.text(
                    center,
                    Align2::CENTER_CENTER,
                    &slices[idx].label,
                    m.font_id(FontSize::Pt8),
                    accent_col,
                );
            } else {
                painter.text(
                    pos2(center.x, center.y - m.u(0.15)),
                    Align2::CENTER_CENTER,
                    "DAWN",
                    m.font_id(FontSize::Pt8),
                    accent_col,
                );
                painter.text(
                    pos2(center.x, center.y + m.u(0.22)),
                    Align2::CENTER_CENTER,
                    "PIE",
                    m.font_id(FontSize::Pt6),
                    text_col.gamma_multiply(0.65),
                );
            }
        });

    if close {
        ctx.data_mut(|d| d.remove::<PieAnimState>(state_id));
    } else {
        ctx.data_mut(|d| d.insert_temp(state_id, state));
    }

    PieResult { selected, close }
}

/// Paints a filled annular sector (wedge) between `r0` and `r1` from angle `a0` to `a1`.
fn paint_annular_wedge(
    painter: &Painter,
    center: Pos2,
    r0: f32,
    r1: f32,
    a0: f32,
    a1: f32,
    fill: Color32,
) {
    let mut mesh = Mesh::default();
    for step in 0..=ARC_STEPS {
        let t = step as f32 / ARC_STEPS as f32;
        let a = a0 + (a1 - a0) * t;
        let (sin, cos) = a.sin_cos();
        let p_in = pos2(center.x + r0 * cos, center.y + r0 * sin);
        let p_out = pos2(center.x + r1 * cos, center.y + r1 * sin);
        let base = mesh.vertices.len() as u32;
        mesh.vertices.push(Vertex {
            pos: p_in,
            uv: egui::epaint::WHITE_UV,
            color: fill,
        });
        mesh.vertices.push(Vertex {
            pos: p_out,
            uv: egui::epaint::WHITE_UV,
            color: fill,
        });
        if step > 0 {
            mesh.indices
                .extend_from_slice(&[base - 2, base - 1, base, base - 1, base + 1, base]);
        }
    }
    painter.add(Shape::mesh(mesh));
}

/// Paints an arc stroke of radius `r` from angle `a0` to `a1`.
fn paint_arc(painter: &Painter, center: Pos2, r: f32, a0: f32, a1: f32, stroke: Stroke) {
    let points: Vec<Pos2> = (0..=ARC_STEPS)
        .map(|s| {
            let a = a0 + (a1 - a0) * (s as f32 / ARC_STEPS as f32);
            pos2(center.x + r * a.cos(), center.y + r * a.sin())
        })
        .collect();
    painter.add(Shape::line(points, stroke));
}

/// MorningDAW 8-slice Studio Quick Radial Pie operations.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuickPieOp {
    Arrange,
    PianoEdit,
    ChannelRack,
    Mixer,
    AddTrack,
    NewClip,
    CloneTrack,
    CommandPalette,
}

/// The 8 slices of the global MorningDAW Studio Radial Pie Menu.
pub fn studio_pie_slices(can_clone: bool) -> Vec<PieSlice<QuickPieOp>> {
    vec![
        PieSlice::new(QuickPieOp::Arrange, "Arrange", Some(IconId::Tracks)).shortcut("1"),
        PieSlice::new(QuickPieOp::PianoEdit, "Piano / Edit", Some(IconId::Piano)).shortcut("2"),
        PieSlice::new(QuickPieOp::ChannelRack, "Devices", Some(IconId::RackTab)).shortcut("3"),
        PieSlice::new(QuickPieOp::Mixer, "Mixer", Some(IconId::Mixer)).shortcut("4"),
        PieSlice::new(QuickPieOp::AddTrack, "Add Track", Some(IconId::AddLarge)).shortcut("N"),
        PieSlice::new(QuickPieOp::NewClip, "New Clip", Some(IconId::PatternClip)).shortcut("C"),
        PieSlice::new(QuickPieOp::CloneTrack, "Clone", Some(IconId::Swap))
            .shortcut("D")
            .enabled(can_clone),
        PieSlice::new(QuickPieOp::CommandPalette, "Commands", Some(IconId::Search))
            .shortcut("Ctrl+P"),
    ]
}

/// Converts Track Header [`TrackOp`] items into up to 8 radial pie slices for tactile track control.
pub fn track_pie_slices(items: &[crate::ui::popup::PopupItem<TrackOp>]) -> Vec<PieSlice<TrackOp>> {
    items
        .iter()
        .filter(|i| !i.is_header())
        .take(8)
        .filter_map(|item| {
            let op = item.id?;
            let icon = match op {
                TrackOp::ToggleZip | TrackOp::ToggleHidden => Some(IconId::Hide),
                TrackOp::ToggleCollapsed | TrackOp::CollapseAll | TrackOp::ExpandAll => {
                    Some(IconId::Collapse)
                }
                TrackOp::ChannelSettings => Some(IconId::Knob),
                TrackOp::CloneChannel => Some(IconId::Swap),
                TrackOp::InsertChannel
                | TrackOp::AddTrack
                | TrackOp::AddNotesTrack
                | TrackOp::AddStepSeqTrack => Some(IconId::AddLarge),
                TrackOp::DeleteTrack | TrackOp::DeleteChannel => Some(IconId::Close),
                TrackOp::SaveMidi | TrackOp::SaveDrumset => Some(IconId::Download),
                TrackOp::LoadDrumset | TrackOp::ImportMidi => Some(IconId::Import),
                _ => Some(IconId::Tracks),
            };
            let short_label = match op {
                TrackOp::ChannelSettings => "Settings".to_owned(),
                TrackOp::CloneChannel => "Clone".to_owned(),
                TrackOp::InsertChannel => "Insert Ch".to_owned(),
                TrackOp::DeleteChannel => "Delete Ch".to_owned(),
                TrackOp::DeleteTrack => "Del Track".to_owned(),
                TrackOp::AddNotesTrack => "Add Notes".to_owned(),
                TrackOp::AddStepSeqTrack => "Add Drums".to_owned(),
                TrackOp::ToggleCollapsed => "Collapse".to_owned(),
                TrackOp::ToggleHidden => "Hide".to_owned(),
                TrackOp::ToggleZip => "Sub-Tracks".to_owned(),
                _ => item.label.to_string(),
            };
            Some(
                PieSlice::new(op, short_label, icon)
                    .enabled(!item.disabled),
            )
        })
        .collect()
}

/// Actions dispatched by the MorningDAW Radial Pie Menu or `Ctrl+K` Command Palette.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DawnAction {
    Nav(Nav),
    /// Opens the Piano Roll / Drum / Clip editor for the selected channel, creating a 1-bar clip first if empty.
    EditOrCreateClip,
    /// Creates a new 1-bar clip on the selected channel's main lane at the playhead.
    NewClipAtPlayhead,
    /// Opens the Add Track / Instrument chooser popup.
    OpenAddTrack,
    /// Directly adds a channel or Lemon Studio instrument plugin.
    AddChannel(AddChannelPick),
    /// Clones the selected channel.
    CloneSelectedChannel,
    /// Switches the active theme JSON (`None` = Classic Gray built-in theme 0).
    SetThemeJson(Option<&'static str>),
}

/// Persistent state for DawnStudio's Radial Pie Menu & `Ctrl+K` Command Palette.
#[derive(Default)]
pub struct DawnQuickState {
    /// Center position where the global Studio Radial Pie Menu is open.
    pub pie_open: Option<Pos2>,
    /// Whether the Track Header menu is currently rendered in Radial Pie mode.
    pub track_pie_mode: bool,
    /// Whether the `Ctrl+K` MorningDAW Command Palette is open.
    pub palette_open: bool,
    /// Search query inside the Command Palette.
    pub query: String,
    /// Selected row index inside the Command Palette.
    pub selected_row: usize,
}

struct PaletteEntry {
    category: &'static str,
    title: &'static str,
    shortcut: &'static str,
    icon: IconId,
    action: DawnAction,
}

fn palette_entries(selected_channel: usize) -> Vec<PaletteEntry> {
    let lemon = |name: &str| {
        let uid = crate::plugins::builtin::named(name).uid;
        DawnAction::AddChannel(AddChannelPick::Plugin(uid))
    };
    vec![
        PaletteEntry {
            category: "VIEW",
            title: "View: Arrange / Playlist",
            shortcut: "1",
            icon: IconId::Tracks,
            action: DawnAction::Nav(Nav::Show(View::Playlist)),
        },
        PaletteEntry {
            category: "VIEW",
            title: "View: Piano Roll / Pattern Editor",
            shortcut: "2",
            icon: IconId::Piano,
            action: DawnAction::EditOrCreateClip,
        },
        PaletteEntry {
            category: "VIEW",
            title: "View: Toggle Channel Rack (Devices & FX)",
            shortcut: "3",
            icon: IconId::RackTab,
            action: DawnAction::Nav(Nav::Toggle(Overlay::Rack(selected_channel))),
        },
        PaletteEntry {
            category: "VIEW",
            title: "View: Multi-Channel Mixer",
            shortcut: "4",
            icon: IconId::Mixer,
            action: DawnAction::Nav(Nav::Show(View::Mixer)),
        },
        PaletteEntry {
            category: "EDIT",
            title: "Clip: Insert New Clip at Playhead",
            shortcut: "C",
            icon: IconId::PatternClip,
            action: DawnAction::NewClipAtPlayhead,
        },
        PaletteEntry {
            category: "EDIT",
            title: "Track: Clone Selected Channel",
            shortcut: "D",
            icon: IconId::Swap,
            action: DawnAction::CloneSelectedChannel,
        },
        PaletteEntry {
            category: "EDIT",
            title: "Project: Tempo & Time Signature",
            shortcut: "T",
            icon: IconId::Tempo,
            action: DawnAction::Nav(Nav::Toggle(Overlay::TempoEditor)),
        },
        PaletteEntry {
            category: "LEMON",
            title: "Add Lemon Synth: Citrus (6-Op FM Synthesizer)",
            shortcut: "FM",
            icon: IconId::Piano,
            action: lemon("Citrus"),
        },
        PaletteEntry {
            category: "LEMON",
            title: "Add Lemon Synth: Lemon Trio (3x Oscillator)",
            shortcut: "3xOsc",
            icon: IconId::Piano,
            action: lemon("Lemon Trio"),
        },
        PaletteEntry {
            category: "LEMON",
            title: "Add Lemon Synth: Lemon Sampler",
            shortcut: "SMP",
            icon: IconId::AudioClip,
            action: lemon("Lemon Sampler"),
        },
        PaletteEntry {
            category: "LEMON",
            title: "Add Lemon Synth: Lemon Drum Sampler",
            shortcut: "DRM",
            icon: IconId::StepSeq,
            action: lemon("Lemon Drum Sampler"),
        },
        PaletteEntry {
            category: "LEMON",
            title: "Add Lemon Synth: Lemon Slicer",
            shortcut: "SLC",
            icon: IconId::AudioClip,
            action: lemon("Lemon Slicer"),
        },
        PaletteEntry {
            category: "SYNTH",
            title: "Add Synth: SuperSaw",
            shortcut: "SAW",
            icon: IconId::Piano,
            action: DawnAction::AddChannel(AddChannelPick::Kind(ChannelKind::Synth(
                SynthKind::SuperSaw,
            ))),
        },
        PaletteEntry {
            category: "SYNTH",
            title: "Add Synth: MiniSynth",
            shortcut: "MINI",
            icon: IconId::Piano,
            action: DawnAction::AddChannel(AddChannelPick::Kind(ChannelKind::Synth(
                SynthKind::MiniSynth,
            ))),
        },
        PaletteEntry {
            category: "SYNTH",
            title: "Add Track: Drum Sequencer",
            shortcut: "KIT",
            icon: IconId::StepSeq,
            action: DawnAction::AddChannel(AddChannelPick::Kind(ChannelKind::Drums)),
        },
        PaletteEntry {
            category: "THEME",
            title: "Theme: Morning Ember (MorningDAW Default)",
            shortcut: "Amber",
            icon: IconId::FavoriteOn,
            action: DawnAction::SetThemeJson(Some(MORNING_EMBER_JSON)),
        },
        PaletteEntry {
            category: "THEME",
            title: "Theme: Wyvern Cyan (Electric Dark)",
            shortcut: "Cyan",
            icon: IconId::FavoriteOn,
            action: DawnAction::SetThemeJson(Some(WYVERN_CYAN_JSON)),
        },
        PaletteEntry {
            category: "THEME",
            title: "Theme: Dawn Gold (Warm Studio)",
            shortcut: "Gold",
            icon: IconId::FavoriteOn,
            action: DawnAction::SetThemeJson(Some(DAWN_GOLD_JSON)),
        },
        PaletteEntry {
            category: "THEME",
            title: "Theme: Classic Gray",
            shortcut: "Gray",
            icon: IconId::FavoriteOff,
            action: DawnAction::SetThemeJson(None),
        },
        PaletteEntry {
            category: "MENU",
            title: "Project: Songs & Export Browser",
            shortcut: "Songs",
            icon: IconId::Browser,
            action: DawnAction::Nav(Nav::Open(Overlay::MainMenu(MenuTab::Songs))),
        },
    ]
}

impl DawnQuickState {
    /// Checks global keyboard & mouse triggers (`Ctrl+K` for Command Palette, `Alt+Q` or Middle-Click for Radial Pie)
    /// and renders any open Pie Menu or Command Palette overlay.
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        cx: &UiCtx,
        screen: Rect,
        selected_channel: usize,
    ) -> Option<DawnAction> {
        // Keyboard triggers when no text input has focus.
        if !ctx.wants_keyboard_input() || self.palette_open {
            let open_palette = ctx.input_mut(|i| {
                i.consume_key(egui::Modifiers::COMMAND, Key::P)
                    || i.consume_key(egui::Modifiers::CTRL, Key::P)
                    || i.consume_key(egui::Modifiers::COMMAND | egui::Modifiers::SHIFT, Key::P)
                    || i.consume_key(egui::Modifiers::CTRL | egui::Modifiers::SHIFT, Key::P)
                    || i.consume_key(egui::Modifiers::ALT, Key::K)
            });
            if open_palette {
                self.palette_open = !self.palette_open;
                if self.palette_open {
                    self.pie_open = None;
                    self.query.clear();
                    self.selected_row = 0;
                }
                ctx.request_repaint();
            }
        }
        if !ctx.wants_keyboard_input() && !self.palette_open {
            let alt_q = ctx.input_mut(|i| i.consume_key(egui::Modifiers::ALT, Key::Q));
            let middle_pressed = ctx.input(|i| i.pointer.button_pressed(PointerButton::Middle));
            if alt_q || middle_pressed {
                let anchor = ctx
                    .input(|i| i.pointer.interact_pos().or(i.pointer.latest_pos()))
                    .unwrap_or_else(|| screen.center());
                self.pie_open = if self.pie_open.is_some() && alt_q {
                    None
                } else {
                    Some(anchor)
                };
                ctx.request_repaint();
            }
        }

        let mut action = None;

        if let Some(anchor) = self.pie_open {
            let slices = studio_pie_slices(selected_channel > 0);
            let res = show(
                ctx,
                Id::new("dawn_studio_pie_menu"),
                anchor,
                screen,
                &slices,
                cx,
            );
            if res.close {
                self.pie_open = None;
                ctx.request_repaint();
            }
            if let Some(op) = res.selected {
                action = Some(match op {
                    QuickPieOp::Arrange => DawnAction::Nav(Nav::Show(View::Playlist)),
                    QuickPieOp::PianoEdit => DawnAction::EditOrCreateClip,
                    QuickPieOp::ChannelRack => {
                        DawnAction::Nav(Nav::Toggle(Overlay::Rack(selected_channel)))
                    }
                    QuickPieOp::Mixer => DawnAction::Nav(Nav::Show(View::Mixer)),
                    QuickPieOp::AddTrack => DawnAction::OpenAddTrack,
                    QuickPieOp::NewClip => DawnAction::NewClipAtPlayhead,
                    QuickPieOp::CloneTrack => DawnAction::CloneSelectedChannel,
                    QuickPieOp::CommandPalette => {
                        self.palette_open = true;
                        self.query.clear();
                        self.selected_row = 0;
                        return None;
                    }
                });
            }
        }

        if self.palette_open {
            if let Some(act) = self.show_palette(ctx, cx, screen, selected_channel) {
                action = Some(act);
            }
        }

        action
    }

    fn show_palette(
        &mut self,
        ctx: &egui::Context,
        cx: &UiCtx,
        screen: Rect,
        selected_channel: usize,
    ) -> Option<DawnAction> {
        let (c, m) = (&cx.theme.colors, cx.m);
        let mut chosen = None;
        let mut should_close = false;

        let entries = palette_entries(selected_channel);
        let q = self.query.trim().to_ascii_lowercase();
        let filtered: Vec<&PaletteEntry> = entries
            .iter()
            .filter(|e| {
                q.is_empty()
                    || e.title.to_ascii_lowercase().contains(&q)
                    || e.category.to_ascii_lowercase().contains(&q)
                    || e.shortcut.to_ascii_lowercase().contains(&q)
            })
            .collect();

        if !filtered.is_empty() {
            self.selected_row = self.selected_row.min(filtered.len() - 1);
        } else {
            self.selected_row = 0;
        }

        let modal_w = m.u(19.0).min(screen.width() - m.u(1.5));
        let modal_h = m.u(12.5).min(screen.height() - m.u(1.5));
        let modal_rect = Rect::from_center_size(screen.center(), vec2(modal_w, modal_h));

        egui::Area::new(Id::new("dawn_command_palette"))
            .order(Order::Foreground)
            .fixed_pos(screen.min)
            .constrain(false)
            .fade_in(false)
            .show(ctx, |ui| {
                let backdrop = ui.allocate_rect(screen, Sense::click());
                let painter =
                    ctx.layer_painter(LayerId::new(Order::Foreground, Id::new("palette_paint")));
                painter.rect_filled(screen, 0.0, Color32::from_black_alpha(130));

                if backdrop.clicked()
                    && backdrop
                        .interact_pointer_pos()
                        .is_some_and(|p| !modal_rect.contains(p))
                {
                    should_close = true;
                }

                // Modal card background & MorningDAW amber top border.
                let corner = m.u(0.25);
                painter.rect_filled(modal_rect, corner, c.menu_fill.to_color32());
                painter.rect_stroke(
                    modal_rect,
                    corner,
                    Stroke::new(1.5 / m.ppp(), c.accent.to_color32().gamma_multiply(0.8)),
                    StrokeKind::Inside,
                );

                // Quick MorningDAW View Switcher Strip at top of palette.
                let top_strip = Rect::from_min_max(
                    modal_rect.min + vec2(m.u(0.4), m.u(0.3)),
                    pos2(modal_rect.right() - m.u(0.4), modal_rect.top() + m.u(1.3)),
                );
                let tabs = [
                    ("1 ARRANGE", DawnAction::Nav(Nav::Show(View::Playlist))),
                    ("2 PIANO", DawnAction::EditOrCreateClip),
                    (
                        "3 RACK",
                        DawnAction::Nav(Nav::Toggle(Overlay::Rack(selected_channel))),
                    ),
                    ("4 MIXER", DawnAction::Nav(Nav::Show(View::Mixer))),
                    ("+ SYNTH", DawnAction::OpenAddTrack),
                ];
                let tab_w = top_strip.width() / tabs.len() as f32;
                for (i, (label, act)) in tabs.into_iter().enumerate() {
                    let r = Rect::from_min_size(
                        pos2(top_strip.left() + i as f32 * tab_w, top_strip.top()),
                        vec2(tab_w - m.u(0.12), top_strip.height()),
                    );
                    let resp = ui.interact(r, Id::new(("palette_tab", i)), Sense::click());
                    let fill = if resp.hovered() {
                        c.accent.to_color32()
                    } else {
                        c.control_fill.to_color32()
                    };
                    let fg = if resp.hovered() {
                        c.text_on_accent.to_color32()
                    } else {
                        c.text.to_color32()
                    };
                    painter.rect_filled(r, m.u(0.14), fill);
                    painter.text(
                        r.center(),
                        Align2::CENTER_CENTER,
                        label,
                        m.font_id(FontSize::Pt8),
                        fg,
                    );
                    if resp.clicked() {
                        chosen = Some(act);
                        should_close = true;
                    }
                }

                // Search bar box.
                let search_rect = Rect::from_min_max(
                    pos2(modal_rect.left() + m.u(0.4), top_strip.bottom() + m.u(0.2)),
                    pos2(
                        modal_rect.right() - m.u(0.4),
                        top_strip.bottom() + m.u(1.35),
                    ),
                );
                painter.rect_filled(search_rect, m.u(0.14), c.background.to_color32());
                let icon_rect = Rect::from_min_size(
                    search_rect.min + vec2(m.u(0.15), m.u(0.15)),
                    Vec2::splat(m.u(0.85)),
                );
                draw_icon(
                    &painter,
                    icon_rect,
                    IconId::Search,
                    c.accent.to_color32(),
                    0.55,
                    m,
                );

                let text_rect = search_rect
                    .with_min_x(icon_rect.right() + m.u(0.15))
                    .shrink2(vec2(m.u(0.2), m.u(0.15)));
                let mut child = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(text_rect)
                        .layout(egui::Layout::left_to_right(egui::Align::Center)),
                );
                let edit = TextEdit::singleline(&mut self.query)
                    .hint_text("Type a command, Lemon synth, view, or theme… (Ctrl+K)")
                    .font(m.font_id(FontSize::Pt10))
                    .text_color(c.text.to_color32())
                    .frame(false)
                    .desired_width(text_rect.width());
                let edit_resp = child.add(edit);
                edit_resp.request_focus();

                // Keyboard navigation inside Command Palette.
                if ui.input(|i| i.key_pressed(Key::Escape)) {
                    should_close = true;
                } else if ui.input(|i| i.key_pressed(Key::ArrowDown)) && !filtered.is_empty() {
                    self.selected_row = (self.selected_row + 1) % filtered.len();
                } else if ui.input(|i| i.key_pressed(Key::ArrowUp)) && !filtered.is_empty() {
                    self.selected_row =
                        (self.selected_row + filtered.len() - 1) % filtered.len();
                } else if ui.input(|i| i.key_pressed(Key::Enter))
                    && let Some(entry) = filtered.get(self.selected_row)
                {
                    chosen = Some(entry.action.clone());
                    should_close = true;
                }

                // Filtered command list.
                let list_top = search_rect.bottom() + m.u(0.25);
                let row_h = m.u(1.15);
                let max_rows = ((modal_rect.bottom() - m.u(0.3) - list_top) / row_h)
                    .floor()
                    .max(1.0) as usize;

                for (vis_i, entry) in filtered.iter().take(max_rows).enumerate() {
                    let row_rect = Rect::from_min_size(
                        pos2(modal_rect.left() + m.u(0.4), list_top + vis_i as f32 * row_h),
                        vec2(modal_rect.width() - m.u(0.8), row_h - m.u(0.08)),
                    );
                    let resp =
                        ui.interact(row_rect, Id::new(("palette_row", vis_i)), Sense::click());
                    if resp.hovered() {
                        self.selected_row = vis_i;
                    }
                    let is_sel = self.selected_row == vis_i;
                    let bg = if is_sel {
                        c.accent.to_color32().gamma_multiply(0.24)
                    } else {
                        c.control_fill.to_color32().gamma_multiply(0.6)
                    };
                    painter.rect_filled(row_rect, m.u(0.12), bg);
                    let row_icon = Rect::from_min_size(
                        row_rect.min + vec2(m.u(0.2), m.u(0.12)),
                        Vec2::splat(m.u(0.8)),
                    );
                    let fg = if is_sel {
                        c.accent.to_color32()
                    } else {
                        c.text.to_color32()
                    };
                    draw_icon(&painter, row_icon, entry.icon, fg, 0.55, m);
                    painter.text(
                        pos2(row_icon.right() + m.u(0.25), row_rect.center().y),
                        Align2::LEFT_CENTER,
                        entry.title,
                        m.font_id(FontSize::Pt10),
                        fg,
                    );
                    painter.text(
                        pos2(row_rect.right() - m.u(0.3), row_rect.center().y),
                        Align2::RIGHT_CENTER,
                        entry.shortcut,
                        m.font_id(FontSize::Pt8),
                        fg.gamma_multiply(0.7),
                    );
                    if resp.clicked() {
                        chosen = Some(entry.action.clone());
                        should_close = true;
                    }
                }
            });

        if should_close {
            self.palette_open = false;
            ctx.request_repaint();
        }
        chosen
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hovered_slice_maps_compass_directions_to_wedges() {
        let center = pos2(500.0, 300.0);
        let (inner, outer) = (40.0, 130.0);
        // Inside the inner hub: None.
        assert_eq!(
            hovered_slice(center, pos2(500.0, 280.0), 8, inner, outer),
            None
        );
        // North (top): slice 0.
        assert_eq!(
            hovered_slice(center, pos2(500.0, 210.0), 8, inner, outer),
            Some(0)
        );
        // East (right): slice 2.
        assert_eq!(
            hovered_slice(center, pos2(590.0, 300.0), 8, inner, outer),
            Some(2)
        );
        // South (bottom): slice 4.
        assert_eq!(
            hovered_slice(center, pos2(500.0, 390.0), 8, inner, outer),
            Some(4)
        );
        // West (left): slice 6.
        assert_eq!(
            hovered_slice(center, pos2(410.0, 300.0), 8, inner, outer),
            Some(6)
        );
    }

    #[test]
    fn studio_pie_slices_has_eight_morning_daw_wedges() {
        let slices = studio_pie_slices(true);
        assert_eq!(slices.len(), 8);
        assert_eq!(slices[0].id, QuickPieOp::Arrange);
        assert_eq!(slices[1].id, QuickPieOp::PianoEdit);
        assert_eq!(slices[2].id, QuickPieOp::ChannelRack);
        assert_eq!(slices[3].id, QuickPieOp::Mixer);
        assert_eq!(slices[7].id, QuickPieOp::CommandPalette);
    }
}
