//! Explorer view: browse every parsed field of a save as tables.

use eframe::egui;
use egui_extras::{Column, TableBuilder};
use guild_save_edit::explore::{self, Context, Explored, Step};
use serde_json::Value;
use std::path::PathBuf;

const LINK: egui::Color32 = egui::Color32::from_rgb(90, 170, 255);
const DIM: egui::Color32 = egui::Color32::from_rgb(140, 140, 140);
const ROW_H: f32 = 20.0;
/// Columns are collected from this many records of a list.
const COLUMN_SAMPLE: usize = 200;
const CELL_CHARS: usize = 48;

#[derive(Default)]
pub struct Explorer {
    pub path_input: String,
    data: Option<Explored>,
    nav: Vec<Step>,
    filter: String,
    error: Option<String>,
    cache: Option<Cache>,
    /// Also show binary data, offsets and technical sections.
    show_all: bool,
}

/// Rows/columns of the current table; rebuilt when the view or filter changes.
struct Cache {
    nav: Vec<Step>,
    filter: String,
    show_all: bool,
    columns: Vec<String>,
    rows: Vec<usize>,
}

impl Explorer {
    pub fn open(&mut self, path: PathBuf) {
        self.path_input = path.display().to_string();
        match explore::load(&path) {
            Ok(d) => {
                self.data = Some(d);
                self.nav = vec![Step::Key("header".into())];
                self.filter.clear();
                self.error = None;
                self.cache = None;
            }
            Err(e) => self.error = Some(format!("{e:#}")),
        }
    }

    pub fn show(&mut self, root: &mut egui::Ui) {
        egui::Panel::top("explorer_top").show(root, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label("File:");
                let w = (ui.available_width() - 150.0).max(200.0);
                let resp = ui.add(egui::TextEdit::singleline(&mut self.path_input).desired_width(w));
                let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if ui.button("Browse…").clicked()
                    && let Some(p) = rfd::FileDialog::new()
                        .add_filter("Gilde world files", &["SAV", "sav", "SRV", "srv", "cty", "CTY"])
                        .pick_file()
                {
                    self.open(p);
                }
                if (ui.button("Open").clicked() || enter) && !self.path_input.is_empty() {
                    self.open(PathBuf::from(&self.path_input));
                }
            });
            if let Some(e) = &self.error {
                ui.colored_label(egui::Color32::from_rgb(230, 90, 80), e);
            }
            ui.add_space(4.0);
        });

        let Some(data) = &self.data else {
            egui::CentralPanel::default().show(root, |ui| {
                ui.centered_and_justified(|ui| {
                    ui.label("Open a .SAV, .SRV or .cty file to explore its contents.");
                });
            });
            return;
        };

        let mut go: Option<Vec<Step>> = None;
        let mut show_all = self.show_all;
        let side = egui::Panel::left("explorer_sections").resizable(true).default_size(210.0).show(root, |ui| {
            ui.add_space(4.0);
            ui.strong("Sections");
            ui.add_space(2.0);
            egui::ScrollArea::vertical().show(ui, |ui| {
                if let Value::Object(m) = &data.root {
                    for (k, v) in m {
                        if !self.show_all && explore::is_technical_section(k) {
                            continue;
                        }
                        let selected = matches!(self.nav.first(), Some(Step::Key(s)) if s == k);
                        let label = format!("{k}  ·  {}", explore::type_name(v));
                        if ui.selectable_label(selected, label).clicked() {
                            go = Some(vec![Step::Key(k.clone())]);
                        }
                    }
                }
                ui.allocate_space(ui.available_size());
            });
        });
        side.response.context_menu(|ui| options_menu(ui, &mut show_all));

        let central = egui::CentralPanel::default().show(root, |ui| {
            // Breadcrumb.
            ui.horizontal_wrapped(|ui| {
                if ui.link("save").clicked() {
                    go = Some(Vec::new());
                }
                for (i, s) in self.nav.iter().enumerate() {
                    ui.label("›");
                    if ui.link(s.to_string()).clicked() {
                        go = Some(self.nav[..=i].to_vec());
                    }
                }
            });
            ui.horizontal(|ui| {
                ui.label("Filter:");
                ui.add(egui::TextEdit::singleline(&mut self.filter).hint_text("text in any field").desired_width(260.0));
                if !self.filter.is_empty() && ui.small_button("×").clicked() {
                    self.filter.clear();
                }
                if let Some(c) = &self.cache {
                    ui.colored_label(DIM, format!("{} row(s)", c.rows.len()));
                }
                if !self.nav.is_empty() && ui.button("↑ up").clicked() {
                    go = Some(self.nav[..self.nav.len() - 1].to_vec());
                }
                ui.colored_label(DIM, if self.show_all { "showing everything" } else { "right-click for options" });
            });
            ui.separator();

            let Some(node) = explore::get(&data.root, &self.nav) else {
                ui.label("(path not found)");
                return;
            };
            let stale = self
                .cache
                .as_ref()
                .is_none_or(|c| c.nav != self.nav || c.filter != self.filter || c.show_all != self.show_all);
            if stale {
                self.cache = Some(build_cache(&data.ctx, node, &self.nav, &self.filter, self.show_all));
            }
            let cache = self.cache.as_ref().unwrap();
            if let Some(next) = node_view(ui, &data.ctx, node, &self.nav, cache) {
                go = Some(next);
            }
        });

        central.response.context_menu(|ui| options_menu(ui, &mut show_all));

        if let Some(nav) = go {
            self.nav = nav;
            self.filter.clear();
        }
        if show_all != self.show_all {
            self.show_all = show_all;
            // Leave hidden data when switching back to the simple view.
            if !show_all && let Some(d) = &self.data {
                while !self.nav.is_empty() && explore::path_is_technical(&d.root, &self.nav) {
                    self.nav.pop();
                }
                if self.nav.is_empty() {
                    self.nav = vec![Step::Key("header".into())];
                }
            }
        }
    }
}

fn options_menu(ui: &mut egui::Ui, show_all: &mut bool) {
    ui.checkbox(show_all, "Show everything")
        .on_hover_text("Also show binary data, file offsets and the technical sections (file, scene)");
}

fn matches(text: &str, filter: &str) -> bool {
    filter.is_empty() || text.to_lowercase().contains(filter)
}

fn build_cache(ctx: &Context, node: &Value, nav: &[Step], filter: &str, show_all: bool) -> Cache {
    let f = filter.to_lowercase();
    let scope = explore::scope_of(nav);
    let shown = |k: &str, v: &Value| explore::display(ctx, &scope, k, v).unwrap_or_else(|| explore::preview(v));
    let mut columns = Vec::new();
    let rows = match node {
        Value::Object(m) => m
            .iter()
            .enumerate()
            .filter(|(_, (k, v))| show_all || !explore::is_technical_field(k, v))
            .filter(|(_, (k, v))| {
                matches(k, &f) || matches(explore::label(&scope, k).unwrap_or(""), &f) || matches(&shown(k, v), &f)
            })
            .map(|(i, _)| i)
            .collect(),
        Value::Array(a) if a.first().is_some_and(Value::is_object) => {
            for r in a.iter().take(COLUMN_SAMPLE) {
                if let Value::Object(m) = r {
                    for k in m.keys() {
                        if !columns.contains(k) {
                            columns.push(k.clone());
                        }
                    }
                }
            }
            if !show_all {
                // Hide a column if it holds technical data in every sampled record.
                columns.retain(|c| {
                    a.iter().take(COLUMN_SAMPLE).any(|r| r.get(c).is_some_and(|v| !explore::is_technical_field(c, v)))
                });
            }
            a.iter()
                .enumerate()
                .filter(|(_, r)| match r {
                    Value::Object(m) => {
                        f.is_empty() || columns.iter().any(|c| m.get(c).is_some_and(|v| matches(&shown(c, v), &f)))
                    }
                    other => matches(&shown("", other), &f),
                })
                .map(|(i, _)| i)
                .collect()
        }
        Value::Array(a) => match explore::as_bytes(node) {
            Some(b) => (0..b.len().div_ceil(16)).collect(),
            None => a.iter().enumerate().filter(|(_, v)| matches(&shown("", v), &f)).map(|(i, _)| i).collect(),
        },
        _ => vec![0],
    };
    Cache { nav: nav.to_vec(), filter: filter.to_owned(), show_all, columns, rows }
}

fn short(s: String) -> String {
    if s.chars().count() > CELL_CHARS {
        let t: String = s.chars().take(CELL_CHARS - 1).collect();
        format!("{t}…")
    } else {
        s
    }
}

/// One value cell. Returns true if it was clicked to drill in.
fn value_cell(ui: &mut egui::Ui, ctx: &Context, scope: &str, key: &str, v: &Value) -> bool {
    if let Some(t) = explore::display(ctx, scope, key, v) {
        let r = ui.add(egui::Label::new(short(t.clone())).truncate());
        let mut tip = format!("{t}\n\nraw: {}", explore::preview(v));
        if let Some(x) = explore::int_readings(v) {
            tip += &format!("\n{x}");
        }
        r.on_hover_text(tip);
        return false;
    }
    let text = short(explore::preview(v));
    if explore::is_composite(v) || explore::as_bytes(v).is_some_and(|b| b.len() > 32) {
        let r = ui.add(egui::Label::new(egui::RichText::new(text).color(LINK)).sense(egui::Sense::click()).truncate());
        let r = r.on_hover_text("open");
        return r.clicked();
    }
    let r = ui.add(egui::Label::new(egui::RichText::new(text).monospace()).truncate());
    if let Some(t) = explore::int_readings(v) {
        r.on_hover_text(t);
    } else if let Some(b) = explore::as_bytes(v) {
        r.on_hover_text(explore::hex(&b));
    } else if let Value::String(s) = v
        && s.chars().count() > CELL_CHARS
    {
        r.on_hover_text(s.as_str());
    }
    false
}

/// Field name: the known meaning if there is one, raw name on hover.
fn field_name(ui: &mut egui::Ui, scope: &str, key: &str) {
    match explore::label(scope, key) {
        Some(l) => {
            ui.label(l).on_hover_text(key);
        }
        None => {
            ui.colored_label(DIM, key);
        }
    }
}

fn child(nav: &[Step], s: Step) -> Vec<Step> {
    let mut n = nav.to_vec();
    n.push(s);
    n
}

fn node_view(ui: &mut egui::Ui, ctx: &Context, node: &Value, nav: &[Step], cache: &Cache) -> Option<Vec<Step>> {
    let mut go = None;
    egui::ScrollArea::horizontal().auto_shrink([false, false]).show(ui, |ui| match node {
        Value::Object(m) => {
            let entries: Vec<(&String, &Value)> = m.iter().collect();
            let scope = explore::scope_of(nav);
            TableBuilder::new(ui)
                .id_salt("obj")
                .striped(true)
                .resizable(true)
                .column(Column::initial(180.0).clip(true))
                .column(Column::initial(130.0).clip(true))
                .column(Column::remainder().at_least(200.0).clip(true))
                .header(ROW_H, |mut h| {
                    h.col(|ui| {
                        ui.strong("Field");
                    });
                    h.col(|ui| {
                        ui.strong("Type");
                    });
                    h.col(|ui| {
                        ui.strong("Value");
                    });
                })
                .body(|body| {
                    body.rows(ROW_H, cache.rows.len(), |mut row| {
                        let (k, v) = entries[cache.rows[row.index()]];
                        row.col(|ui| {
                            field_name(ui, &scope, k);
                        });
                        row.col(|ui| {
                            ui.colored_label(DIM, explore::type_name(v));
                        });
                        row.col(|ui| {
                            if value_cell(ui, ctx, &scope, k, v) {
                                go = Some(child(nav, Step::Key(k.clone())));
                            }
                        });
                    });
                });
        }
        Value::Array(a) if !cache.columns.is_empty() => {
            TableBuilder::new(ui)
                .id_salt(("records", cache.columns.len()))
                .striped(true)
                .resizable(true)
                .column(Column::initial(56.0))
                .columns(Column::auto_with_initial_suggestion(90.0).at_most(360.0).clip(true), cache.columns.len())
                .header(ROW_H, |mut h| {
                    h.col(|ui| {
                        ui.strong("#");
                    });
                    let scope = explore::scope_of(nav);
                    for c in &cache.columns {
                        h.col(|ui| {
                            match explore::label(&scope, c) {
                                Some(l) => ui.strong(l).on_hover_text(c.as_str()),
                                None => ui.strong(c),
                            };
                        });
                    }
                })
                .body(|body| {
                    let scope = explore::scope_of(nav);
                    body.rows(ROW_H, cache.rows.len(), |mut row| {
                        let i = cache.rows[row.index()];
                        let rec = &a[i];
                        row.col(|ui| {
                            if ui.link(i.to_string()).on_hover_text("open record").clicked() {
                                go = Some(child(nav, Step::Index(i)));
                            }
                        });
                        for c in &cache.columns {
                            row.col(|ui| match rec.get(c) {
                                Some(v) => {
                                    if value_cell(ui, ctx, &scope, c, v) {
                                        go = Some(child(&child(nav, Step::Index(i)), Step::Key(c.clone())));
                                    }
                                }
                                None => {
                                    ui.colored_label(DIM, "–");
                                }
                            });
                        }
                    });
                });
        }
        Value::Array(a) => {
            if let Some(b) = explore::as_bytes(node) {
                TableBuilder::new(ui)
                    .id_salt("bytes")
                    .striped(true)
                    .column(Column::exact(70.0))
                    .column(Column::exact(400.0))
                    .column(Column::remainder())
                    .header(ROW_H, |mut h| {
                        h.col(|ui| {
                            ui.strong("Offset");
                        });
                        h.col(|ui| {
                            ui.strong("Hex");
                        });
                        h.col(|ui| {
                            ui.strong("Text");
                        });
                    })
                    .body(|body| {
                        body.rows(ROW_H, cache.rows.len(), |mut row| {
                            let line = cache.rows[row.index()];
                            let chunk = &b[line * 16..(line * 16 + 16).min(b.len())];
                            row.col(|ui| {
                                ui.monospace(format!("{:#06x}", line * 16));
                            });
                            row.col(|ui| {
                                ui.monospace(explore::hex(chunk));
                            });
                            row.col(|ui| {
                                let t: String =
                                    chunk.iter().map(|&c| if (0x20..0x7f).contains(&c) { c as char } else { '.' }).collect();
                                ui.monospace(t);
                            });
                        });
                    });
                return;
            }
            TableBuilder::new(ui)
                .id_salt("list")
                .striped(true)
                .resizable(true)
                .column(Column::initial(70.0))
                .column(Column::initial(130.0).clip(true))
                .column(Column::remainder().at_least(200.0).clip(true))
                .header(ROW_H, |mut h| {
                    h.col(|ui| {
                        ui.strong("#");
                    });
                    h.col(|ui| {
                        ui.strong("Type");
                    });
                    h.col(|ui| {
                        ui.strong("Value");
                    });
                })
                .body(|body| {
                    body.rows(ROW_H, cache.rows.len(), |mut row| {
                        let i = cache.rows[row.index()];
                        row.col(|ui| {
                            ui.label(i.to_string());
                        });
                        row.col(|ui| {
                            ui.colored_label(DIM, explore::type_name(&a[i]));
                        });
                        row.col(|ui| {
                            if value_cell(ui, ctx, &explore::scope_of(nav), "", &a[i]) {
                                go = Some(child(nav, Step::Index(i)));
                            }
                        });
                    });
                });
        }
        scalar => {
            ui.monospace(explore::preview(scalar));
            if let Some(t) = explore::int_readings(scalar) {
                ui.add_space(6.0);
                ui.monospace(t);
            }
        }
    });
    go
}
