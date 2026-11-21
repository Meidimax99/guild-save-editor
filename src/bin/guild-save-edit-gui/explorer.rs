//! Explorer view: browse every parsed field of a save as tables.

use eframe::egui;
use egui_extras::{Column, TableBuilder};
use guild_save_edit::explore::{self, Explored, Step};
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
}

/// Rows/columns of the current table; rebuilt when the view or filter changes.
struct Cache {
    nav: Vec<Step>,
    filter: String,
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
        egui::Panel::left("explorer_sections").resizable(true).default_size(210.0).show(root, |ui| {
            ui.add_space(4.0);
            ui.strong("Sections");
            ui.add_space(2.0);
            egui::ScrollArea::vertical().show(ui, |ui| {
                if let Value::Object(m) = &data.root {
                    for (k, v) in m {
                        let selected = matches!(self.nav.first(), Some(Step::Key(s)) if s == k);
                        let label = format!("{k}  ·  {}", explore::type_name(v));
                        if ui.selectable_label(selected, label).clicked() {
                            go = Some(vec![Step::Key(k.clone())]);
                        }
                    }
                }
            });
        });

        egui::CentralPanel::default().show(root, |ui| {
            // Breadcrumb.
            ui.horizontal_wrapped(|ui| {
                if ui.link("file").clicked() {
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
            });
            ui.separator();

            let Some(node) = explore::get(&data.root, &self.nav) else {
                ui.label("(path not found)");
                return;
            };
            let stale = self.cache.as_ref().is_none_or(|c| c.nav != self.nav || c.filter != self.filter);
            if stale {
                self.cache = Some(build_cache(node, &self.nav, &self.filter));
            }
            let cache = self.cache.as_ref().unwrap();
            if let Some(next) = node_view(ui, node, &self.nav, cache) {
                go = Some(next);
            }
        });

        if let Some(nav) = go {
            self.nav = nav;
            self.filter.clear();
        }
    }
}

fn matches(text: &str, filter: &str) -> bool {
    filter.is_empty() || text.to_lowercase().contains(filter)
}

fn build_cache(node: &Value, nav: &[Step], filter: &str) -> Cache {
    let f = filter.to_lowercase();
    let mut columns = Vec::new();
    let rows = match node {
        Value::Object(m) => m
            .iter()
            .enumerate()
            .filter(|(_, (k, v))| matches(k, &f) || matches(&explore::preview(v), &f))
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
            a.iter()
                .enumerate()
                .filter(|(_, r)| match r {
                    Value::Object(m) => f.is_empty() || m.values().any(|v| matches(&explore::preview(v), &f)),
                    other => matches(&explore::preview(other), &f),
                })
                .map(|(i, _)| i)
                .collect()
        }
        Value::Array(a) => match explore::as_bytes(node) {
            Some(b) => (0..b.len().div_ceil(16)).collect(),
            None => a.iter().enumerate().filter(|(_, v)| matches(&explore::preview(v), &f)).map(|(i, _)| i).collect(),
        },
        _ => vec![0],
    };
    Cache { nav: nav.to_vec(), filter: filter.to_owned(), columns, rows }
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
fn value_cell(ui: &mut egui::Ui, v: &Value) -> bool {
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

fn child(nav: &[Step], s: Step) -> Vec<Step> {
    let mut n = nav.to_vec();
    n.push(s);
    n
}

fn node_view(ui: &mut egui::Ui, node: &Value, nav: &[Step], cache: &Cache) -> Option<Vec<Step>> {
    let mut go = None;
    egui::ScrollArea::horizontal().auto_shrink([false, false]).show(ui, |ui| match node {
        Value::Object(m) => {
            let entries: Vec<(&String, &Value)> = m.iter().collect();
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
                            ui.label(k);
                        });
                        row.col(|ui| {
                            ui.colored_label(DIM, explore::type_name(v));
                        });
                        row.col(|ui| {
                            if value_cell(ui, v) {
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
                    for c in &cache.columns {
                        h.col(|ui| {
                            ui.strong(c);
                        });
                    }
                })
                .body(|body| {
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
                                    if value_cell(ui, v) {
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
                            if value_cell(ui, &a[i]) {
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
