// SPDX-License-Identifier: GPL-3.0-only
//! The graphics panel: the presets and every module's settings, generated from the schemas
//! (`fork_gfx::ModuleSpec`); nothing here names a module or a setting. It shows each value, where it
//! comes from (the original, the preset or a layer of the file) and whether a change applies in the
//! next frame or at the next start, and turns what the player does into [`Change`]s for the caller
//! to write into the profile's file ([`apply`]).

use egui::{Color32, RichText};
use fork_gfx::{
    Applies, Kind, Layer, ModuleSpec, Place, Presets, Resolved, SettingsFile, Source, Typed, Value,
};

/// What the player changed in one frame of the panel.
#[derive(Clone, Debug, PartialEq)]
pub enum Change {
    /// `preset = "..."`.
    Preset(String),
    /// `module.key = value` in `layer`.
    Set {
        layer: Layer,
        module: &'static str,
        key: &'static str,
        value: Value,
    },
    /// `module.key` taken out of `layer`: the weaker layers decide again.
    Reset {
        layer: Layer,
        module: &'static str,
        key: &'static str,
    },
}

/// Writes `change` into `file`. Whether the file changed.
pub fn apply(file: &mut SettingsFile, change: &Change) -> bool {
    match change {
        Change::Preset(p) => {
            if file.preset() == Some(p.as_str()) {
                return false;
            }
            file.set_preset(p);
            true
        }
        Change::Set {
            layer,
            module,
            key,
            value,
        } => {
            if file.get(layer, module, key) == Some(value) {
                return false;
            }
            file.set(layer, module, key, value);
            true
        }
        Change::Reset { layer, module, key } => file.remove(layer, module, key),
    }
}

/// The layers an edit can go to at `place`, weakest first: everywhere, then the place's world space
/// (outdoors), interiors (inside) and its cell, when the place names them.
pub fn layers_at(place: &Place) -> Vec<Layer> {
    let name = |n: &fork_gfx::PlaceName| {
        n.editor_id
            .clone()
            .or_else(|| n.form_id.map(|id| format!("{id:08X}")))
    };
    let mut out = vec![Layer::Global];
    if !place.interior {
        if let Some(w) = place.worldspace.as_ref().and_then(name) {
            out.push(Layer::Worldspace(w));
        }
    } else {
        out.push(Layer::Interior);
    }
    if let Some(c) = place.cell.as_ref().and_then(name) {
        out.push(Layer::Cell(c));
    }
    out
}

/// A layer as the player reads it.
pub fn layer_label(l: &Layer) -> String {
    match l {
        Layer::Global => "Everywhere".to_owned(),
        Layer::Worldspace(w) => format!("Outdoors in {w}"),
        Layer::Interior => "Every interior".to_owned(),
        Layer::Cell(c) => format!("This cell ({c})"),
    }
}

/// Where a value comes from, short.
pub fn source_label(s: &Source) -> String {
    match s {
        Source::Original => "original".to_owned(),
        Source::Preset(p, _) => format!("preset {p}"),
        Source::File(l) => l.name(),
        Source::Override(_) => "this run".to_owned(),
    }
}

/// What the panel reads: the schemas, presets, the values resolved at the place and the values the
/// game started with (a `next_start` setting that differs from them is marked).
pub struct View<'a> {
    pub specs: &'a [&'static ModuleSpec],
    pub presets: &'a Presets,
    pub resolved: &'a Resolved,
    pub started: Option<&'a Resolved>,
    pub file: Option<&'a SettingsFile>,
    pub place: &'a Place,
}

/// The panel's own state between frames.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Panel {
    /// Which of [`layers_at`] edits go to (0: everywhere).
    pub layer: usize,
    /// The last line to show under the list (saved, or why not).
    pub status: Option<String>,
}

/// The panel's window title.
pub const TITLE: &str = "Graphics (F10 closes)";

impl Panel {
    /// Draws the panel for one frame; returns what the player changed.
    pub fn show(&mut self, ctx: &egui::Context, view: &View<'_>) -> Vec<Change> {
        let mut changes = Vec::new();
        let layers = layers_at(view.place);
        self.layer = self.layer.min(layers.len() - 1);
        egui::Window::new(TITLE)
            .id(egui::Id::unique("nv-rs graphics overlay"))
            .default_pos(egui::pos2(24.0, 24.0))
            .collapsible(false)
            .resizable(false)
            // One row per setting: on a small frame the list scrolls instead of running off the
            // bottom of the screen.
            .vscroll(true)
            .max_height((ctx.content_rect().height() - 96.0).max(120.0))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Preset");
                    let now = view.resolved.preset.clone();
                    let label = |id: &str| {
                        view.presets
                            .get(id)
                            .map_or_else(|| id.to_owned(), |p| p.label.clone())
                    };
                    egui::ComboBox::from_id_salt("preset")
                        .selected_text(label(&now))
                        .show_ui(ui, |ui| {
                            for p in view.presets.list() {
                                if ui.selectable_label(p.id == now, &p.label).clicked()
                                    && p.id != now
                                {
                                    changes.push(Change::Preset(p.id.clone()));
                                }
                            }
                        });
                });
                ui.horizontal(|ui| {
                    ui.label("Changes go to");
                    egui::ComboBox::from_id_salt("layer")
                        .selected_text(layer_label(&layers[self.layer]))
                        .show_ui(ui, |ui| {
                            for (i, l) in layers.iter().enumerate() {
                                ui.selectable_value(&mut self.layer, i, layer_label(l));
                            }
                        });
                });
                let layer = &layers[self.layer];
                ui.separator();
                // One grid for every module, so the columns line up.
                egui::Grid::new("settings")
                    .num_columns(4)
                    .spacing([12.0, 4.0])
                    .show(ui, |ui| {
                        for spec in view.specs {
                            let Some(m) = view.resolved.module(spec.id) else {
                                continue;
                            };
                            ui.label(RichText::new(spec.label).strong())
                                .on_hover_text(spec.help);
                            ui.end_row();
                            for (i, s) in spec.settings.iter().enumerate() {
                                let now = m.values[i];
                                ui.label(s.label).on_hover_text(s.help);
                                if let Some(v) = setting_widget(ui, &s.kind, now, spec.id, s.key) {
                                    changes.push(Change::Set {
                                        layer: layer.clone(),
                                        module: spec.id,
                                        key: s.key,
                                        value: v,
                                    });
                                }
                                let mut note = source_label(&m.sources[i]);
                                let started = view
                                    .started
                                    .and_then(|r| r.module(spec.id))
                                    .map(|sm| sm.values[i]);
                                if s.applies == Applies::NextStart
                                    && started.is_some_and(|v| v != now)
                                {
                                    note.push_str(", next start");
                                }
                                ui.label(RichText::new(note).color(Color32::GRAY));
                                let here = view.file.and_then(|f| f.get(layer, spec.id, s.key));
                                if here.is_some()
                                    && ui
                                        .small_button("Reset")
                                        .on_hover_text("Take the value out of this layer")
                                        .clicked()
                                {
                                    changes.push(Change::Reset {
                                        layer: layer.clone(),
                                        module: spec.id,
                                        key: s.key,
                                    });
                                } else if here.is_none() {
                                    ui.label("");
                                }
                                ui.end_row();
                            }
                        }
                    });
                if let Some(s) = &self.status {
                    ui.separator();
                    ui.label(RichText::new(s).color(Color32::GRAY));
                }
            });
        changes
    }
}

/// The widget of one setting; the new value when the player changed it.
fn setting_widget(
    ui: &mut egui::Ui,
    kind: &Kind,
    now: Typed,
    module: &str,
    key: &str,
) -> Option<Value> {
    match (kind, now) {
        (Kind::Toggle { .. }, Typed::Toggle(mut b)) => {
            let text = if b { "on" } else { "off" };
            ui.checkbox(&mut b, text)
                .changed()
                .then_some(Value::Bool(b))
        }
        (Kind::Slider { min, max, step, .. }, Typed::Slider(mut n)) => {
            let was = n;
            let moved = ui
                .add(egui::Slider::new(&mut n, *min..=*max).step_by(*step))
                .changed();
            // The slider snaps to its steps in floating point (0.7 comes back as
            // 0.7000000000000001): only a move of at least a thousandth of a step is a change, and
            // the value is written as a multiple of the step, to six places.
            let n = ((n / step).round() * step * 1e6).round() / 1e6;
            (moved && (n - was).abs() >= step * 1e-3).then_some(Value::Number(n))
        }
        (Kind::Choice { options, .. }, Typed::Choice(c)) => {
            let mut out = None;
            let label = options.iter().find(|o| o.id == c).map_or(c, |o| o.label);
            egui::ComboBox::from_id_salt((module, key))
                .selected_text(label)
                .show_ui(ui, |ui| {
                    for o in options.iter() {
                        if ui.selectable_label(o.id == c, o.label).clicked() && o.id != c {
                            out = Some(Value::Text(o.id.to_owned()));
                        }
                    }
                });
            out
        }
        // A value of another type than its kind is not made by the resolution.
        _ => {
            ui.label("?");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fork_gfx::PlaceName;

    #[test]
    fn the_layers_follow_the_place() {
        assert_eq!(layers_at(&Place::default()), vec![Layer::Global]);
        let outside = Place {
            worldspace: Some(PlaceName::editor_id("WastelandNV")),
            interior: false,
            cell: Some(PlaceName {
                editor_id: None,
                form_id: Some(0x10_C0DE),
            }),
        };
        assert_eq!(
            layers_at(&outside),
            vec![
                Layer::Global,
                Layer::Worldspace("WastelandNV".into()),
                Layer::Cell("0010C0DE".into())
            ]
        );
        let inside = Place {
            worldspace: None,
            interior: true,
            cell: Some(PlaceName::editor_id("GSDocMitchellHouse")),
        };
        assert_eq!(
            layers_at(&inside),
            vec![
                Layer::Global,
                Layer::Interior,
                Layer::Cell("GSDocMitchellHouse".into())
            ]
        );
    }

    #[test]
    fn a_change_is_written_once_and_a_reset_takes_it_out() {
        let mut f = SettingsFile::parse("# mine\n[global.ao]\nfuture_key = 3 # kept\n");
        let set = Change::Set {
            layer: Layer::Interior,
            module: "ao",
            key: "strength",
            value: Value::Number(0.5),
        };
        assert!(apply(&mut f, &set));
        assert!(!apply(&mut f, &set), "the same value again writes nothing");
        assert!(apply(&mut f, &Change::Preset("modern".into())));
        let text = f.to_text();
        assert!(
            text.contains("future_key = 3 # kept") && text.contains("# mine"),
            "{text}"
        );
        assert_eq!(
            f.get(&Layer::Interior, "ao", "strength"),
            Some(&Value::Number(0.5))
        );
        let reset = Change::Reset {
            layer: Layer::Interior,
            module: "ao",
            key: "strength",
        };
        assert!(apply(&mut f, &reset));
        assert!(!apply(&mut f, &reset));
        assert_eq!(f.get(&Layer::Interior, "ao", "strength"), None);
    }

    /// One frame of the panel with `events`, headless; the changes it returned.
    fn frame(
        ctx: &egui::Context,
        panel: &mut Panel,
        file: &SettingsFile,
        events: Vec<egui::Event>,
    ) -> Vec<Change> {
        let specs = fork_gfx::builtin::specs();
        let presets = fork_gfx::builtin::presets();
        let place = Place::default();
        let resolved = fork_gfx::resolve(
            &fork_gfx::Inputs {
                specs: &specs,
                presets: &presets,
                file: Some(file),
                override_preset: None,
                overrides: None,
            },
            &place,
        );
        let view = View {
            specs: &specs,
            presets: &presets,
            resolved: &resolved,
            started: None,
            file: Some(file),
            place: &place,
        };
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 720.0),
            )),
            events,
            focused: true,
            ..Default::default()
        };
        let mut changes = Vec::new();
        let mut out = ctx.run_pass(input, |ctx| changes = panel.show(ctx, &view));
        out.textures_delta.clear();
        changes
    }

    fn key(key: egui::Key, pressed: bool) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }
    }

    /// The keyboard alone reaches every setting: Tab moves the focus from the title to the preset,
    /// the layer and the first setting (the ambient occlusion switch), Space turns it on, and the panel hands back
    /// that one change for the global layer; nothing else changes on the way.
    #[test]
    fn the_keyboard_turns_a_setting_on() {
        let ctx = crate::painter::context(&[[0b11111; 7]; 95]);
        let mut panel = Panel::default();
        let file = SettingsFile::new();
        let first = frame(&ctx, &mut panel, &file, vec![]);
        assert!(first.is_empty(), "{first:?}");
        for _ in 0..4 {
            let c = frame(
                &ctx,
                &mut panel,
                &file,
                vec![key(egui::Key::Tab, true), key(egui::Key::Tab, false)],
            );
            assert!(c.is_empty(), "{c:?}");
        }
        let c = frame(
            &ctx,
            &mut panel,
            &file,
            vec![key(egui::Key::Space, true), key(egui::Key::Space, false)],
        );
        assert_eq!(
            c,
            vec![Change::Set {
                layer: Layer::Global,
                module: "ao",
                key: "enabled",
                value: Value::Bool(true),
            }]
        );
    }
}
