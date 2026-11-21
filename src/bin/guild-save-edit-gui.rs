//! GUI for `guild-save-edit`.
//!
//! 1. Scan a folder tree with the players' network saves.
//! 2. Pick a save point; every player's copy of it is loaded together.
//! 3. See which players' copies are present or missing, add single files.
//! 4. Tick the players to hand to the AI and write all copies, in place or
//!    into a new folder that mirrors the scanned tree.

use eframe::egui;
use guild_save_edit::ops;
use guild_save_edit::session::{self, Found, ScanResult, Session};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, mpsc};

const RED: egui::Color32 = egui::Color32::from_rgb(230, 90, 80);
const BLUE: egui::Color32 = egui::Color32::from_rgb(90, 170, 255);
const AMBER: egui::Color32 = egui::Color32::from_rgb(230, 170, 60);
const GREEN: egui::Color32 = egui::Color32::from_rgb(110, 190, 110);

fn main() -> eframe::Result {
    let initial = std::env::args().nth(1).map(PathBuf::from);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1100.0, 700.0]).with_title("Gilde Savegame Editor"),
        ..Default::default()
    };
    eframe::run_native(
        "guild-save-edit",
        options,
        Box::new(move |_cc| {
            let mut app = App::default();
            if let Some(p) = initial {
                app.root_input = p.display().to_string();
                if p.is_dir() {
                    app.start_scan(p);
                } else {
                    app.open_single(&p);
                }
            }
            Ok(Box::new(app))
        }),
    )
}

struct Scan {
    root: PathBuf,
    done: Arc<AtomicUsize>,
    total: Arc<AtomicUsize>,
    rx: mpsc::Receiver<ScanResult>,
}

#[derive(PartialEq)]
enum Output {
    InPlace,
    Folder,
}

struct App {
    root_input: String,
    scan: Option<Scan>,
    /// Save points of the last scan.
    found: Vec<Found>,
    scanned_root: Option<PathBuf>,
    session: Option<Session>,
    to_ai: BTreeSet<u16>,
    keep_notices: bool,
    output: Output,
    out_dir: String,
    confirm: bool,
    log: Vec<String>,
}

impl Default for App {
    fn default() -> Self {
        App {
            root_input: String::new(),
            scan: None,
            found: Vec::new(),
            scanned_root: None,
            session: None,
            to_ai: BTreeSet::new(),
            keep_notices: false,
            output: Output::Folder,
            out_dir: String::new(),
            confirm: false,
            log: Vec::new(),
        }
    }
}

impl App {
    fn start_scan(&mut self, root: PathBuf) {
        let (tx, rx) = mpsc::channel();
        let done = Arc::new(AtomicUsize::new(0));
        let total = Arc::new(AtomicUsize::new(0));
        let (d, t, r) = (done.clone(), total.clone(), root.clone());
        std::thread::spawn(move || {
            let res = session::scan(&r, |i, n| {
                d.store(i, Ordering::Relaxed);
                t.store(n, Ordering::Relaxed);
            });
            let _ = tx.send(res);
        });
        self.log.push(format!("scanning {}", root.display()));
        self.session = None;
        self.found.clear();
        self.scan = Some(Scan { root, done, total, rx });
    }

    fn poll_scan(&mut self) {
        let Some(scan) = &self.scan else { return };
        if let Ok(res) = scan.rx.try_recv() {
            let root = scan.root.clone();
            self.log.extend(res.errors.iter().map(|e| format!("error: skipped {e}")));
            let copies: usize = res.found.iter().map(|f| f.copies.len()).sum();
            self.log.push(format!("found {} save(s) in {} file(s)", res.found.len(), copies));
            self.found = res.found;
            self.scanned_root = Some(root.clone());
            if self.out_dir.is_empty() {
                let name = root.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "saves".into());
                self.out_dir = root.with_file_name(format!("{name}-edited")).display().to_string();
            }
            self.scan = None;
        }
    }

    fn open_found(&mut self, idx: usize) {
        let f = self.found.remove(idx);
        let sess = Session::new(f, self.scanned_root.clone());
        self.log.push(format!("opened save “{}” with {} cop(ies)", sess.name(), sess.copies.len()));
        self.to_ai.clear();
        self.session = Some(sess);
    }

    fn open_single(&mut self, path: &Path) {
        match session::Copy::load(path, None) {
            Ok(c) => {
                let key = c.key();
                let sess = Session::new(Found { key, copies: vec![c] }, None);
                self.log.push(format!("opened {}", path.display()));
                if self.out_dir.is_empty()
                    && let Some(p) = path.parent()
                {
                    self.out_dir = p.join("edited").display().to_string();
                }
                self.to_ai.clear();
                self.session = Some(sess);
            }
            Err(e) => self.log.push(format!("error: {e:#}")),
        }
    }

    fn back_to_list(&mut self) {
        // Rescan so the list reflects any files written meanwhile.
        self.session = None;
        if let Some(r) = self.scanned_root.clone() {
            self.start_scan(r);
        }
    }

    // ---------------------------------------------------------------- views

    fn top_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("Folder with the players' saves:");
            let w = (ui.available_width() - 330.0).max(200.0);
            let resp = ui.add(egui::TextEdit::singleline(&mut self.root_input).desired_width(w));
            let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if ui.button("Browse…").clicked()
                && let Some(p) = rfd::FileDialog::new().pick_folder()
            {
                self.root_input = p.display().to_string();
                self.start_scan(p);
            }
            if (ui.add_enabled(self.scan.is_none(), egui::Button::new("Scan")).clicked() || enter) && !self.root_input.is_empty() {
                let p = PathBuf::from(&self.root_input);
                if p.is_dir() {
                    self.start_scan(p);
                } else {
                    self.log.push(format!("error: {} is not a folder", p.display()));
                }
            }
            if ui.button("Open single file…").clicked()
                && let Some(p) = rfd::FileDialog::new().add_filter("Gilde savegame", &["SAV", "sav"]).pick_file()
            {
                self.open_single(&p);
            }
        });
    }

    fn list_view(&mut self, ui: &mut egui::Ui) {
        if let Some(scan) = &self.scan {
            let (d, t) = (scan.done.load(Ordering::Relaxed), scan.total.load(Ordering::Relaxed));
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(if t == 0 { "looking for save files…".to_string() } else { format!("reading save {} of {t}…", d + 1) });
            });
            ui.ctx().request_repaint();
            return;
        }
        if self.found.is_empty() {
            ui.centered_and_justified(|ui| {
                ui.label("Choose the folder that contains the saves you collected from the players.\nSub-folders are searched too.");
            });
            return;
        }
        ui.heading(if self.found.len() == 1 { "1 save found".to_string() } else { format!("{} different saves found — choose one", self.found.len()) });
        ui.label("Copies of the same save from different players are grouped together.");
        ui.add_space(6.0);
        let mut open = None;
        egui::ScrollArea::vertical().show(ui, |ui| {
            egui::Grid::new("found").striped(true).num_columns(7).spacing([16.0, 6.0]).show(ui, |ui| {
                for h in ["Save", "City", "In-game date", "Copies (players)", "Newest file", "Files", ""] {
                    ui.strong(h);
                }
                ui.end_row();
                for (i, f) in self.found.iter().enumerate() {
                    let r = &f.copies[0].sav;
                    ui.label(egui::RichText::new(f.name()).strong());
                    ui.label(&f.key.city);
                    ui.label(session::date_text(r));
                    let mut owners: Vec<String> = f.copies.iter().map(|c| c.owner_name()).collect();
                    owners.sort();
                    owners.dedup();
                    let humans = ops::human_count(r).max(owners.len());
                    ui.label(format!("{} of {humans}", owners.len())).on_hover_text(owners.join("\n"));
                    ui.label(session::time_text(f.newest()));
                    let files: Vec<String> = f
                        .copies
                        .iter()
                        .map(|c| format!("{}  ({}, {})", c.sav.path.display(), c.owner_name(), session::time_text(c.modified)))
                        .collect();
                    ui.label(format!("{} file(s)", f.copies.len())).on_hover_text(files.join("\n"));
                    if ui.button("Open").clicked() {
                        open = Some(i);
                    }
                    ui.end_row();
                }
            });
        });
        if let Some(i) = open {
            self.open_found(i);
        }
    }

    fn session_view(&mut self, ui: &mut egui::Ui) {
        let mut add_files = Vec::new();
        let mut back = false;
        {
            let sess = self.session.as_ref().unwrap();
            let r = sess.reference();
            ui.horizontal(|ui| {
                if self.scanned_root.is_some() && ui.button("← Save list").clicked() {
                    back = true;
                }
                ui.heading(format!("“{}”", sess.name()));
                ui.label(format!("{} — {}", sess.key.city, session::date_text(r)));
            });
            ui.add_space(4.0);

            let seats = sess.seats();
            let missing: Vec<&session::Seat> = seats.iter().filter(|s| s.copy.is_none()).collect();
            ui.horizontal(|ui| {
                ui.label(format!("Copies loaded: {} of {} players", seats.len() - missing.len(), seats.len()));
                if !missing.is_empty() {
                    ui.colored_label(
                        RED,
                        format!("— not yet available: {}", missing.iter().map(|s| s.name.as_str()).collect::<Vec<_>>().join(", ")),
                    );
                } else {
                    ui.colored_label(GREEN, "— complete");
                }
            });
            ui.add_space(4.0);

            egui::ScrollArea::vertical().id_salt("seats").max_height(ui.available_height() - 230.0).show(ui, |ui| {
                egui::Grid::new("seats").striped(true).num_columns(6).spacing([14.0, 5.0]).show(ui, |ui| {
                    for h in ["Hand to AI", "ID", "Player", "Dynasty", "Role", "This player's save file"] {
                        ui.strong(h);
                    }
                    ui.end_row();
                    for s in &seats {
                        let human = session::is_human_kind(s.kind);
                        if human {
                            let mut on = self.to_ai.contains(&s.id);
                            if ui.checkbox(&mut on, "").changed() {
                                if on {
                                    self.to_ai.insert(s.id);
                                } else {
                                    self.to_ai.remove(&s.id);
                                }
                            }
                        } else {
                            ui.label("");
                        }
                        ui.label(s.id.to_string());
                        ui.label(&s.name);
                        ui.label(&s.dynasty);
                        if human {
                            ui.colored_label(BLUE, session::seat_role(s.kind));
                        } else {
                            ui.colored_label(AMBER, "AI player (was human)");
                        }
                        match s.copy {
                            Some(i) => {
                                let c = &sess.copies[i];
                                let mut text = format!("{}   ({})", c.sav.path.display(), session::time_text(c.modified));
                                if !c.twins.is_empty() {
                                    text += &format!("   +{} identical", c.twins.len());
                                }
                                let resp = ui.colored_label(GREEN, text);
                                let mut tip: Vec<String> = Vec::new();
                                if c.srv.is_none() {
                                    tip.push("no .SRV next to it".into());
                                }
                                tip.extend(c.notes.iter().cloned());
                                tip.extend(c.twins.iter().map(|t| format!("identical copy: {}", t.sav.path.display())));
                                if !tip.is_empty() {
                                    resp.on_hover_text(tip.join("\n"));
                                }
                            }
                            None => {
                                ui.colored_label(RED, "missing — not yet available");
                            }
                        }
                        ui.end_row();
                    }
                });
                let ai: Vec<String> = r
                    .world
                    .characters
                    .list
                    .iter()
                    .filter(|c| c.f02 == ops::KIND_AI && !seats.iter().any(|s| s.id == c.id))
                    .map(|c| format!("{} ({})", ops::full_name(c), ops::dynasty_name(r, c)))
                    .collect();
                if !ai.is_empty() {
                    ui.add_space(6.0);
                    ui.label(format!("Computer dynasties: {}", ai.join(", ")));
                }
                for (_, why) in &sess.conflicts {
                    ui.colored_label(RED, format!("not used: {why}"));
                }
            });

            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("Add save file(s)…").on_hover_text("Load more players' copies of this save").clicked()
                    && let Some(files) = rfd::FileDialog::new().add_filter("Gilde savegame", &["SAV", "sav"]).pick_files()
                {
                    add_files = files;
                }
                ui.checkbox(&mut self.keep_notices, "keep pending notices")
                    .on_hover_text("By default, open message boxes of the handed-over player that need a human decision are cancelled, like the game does when a player leaves.");
            });
            ui.horizontal(|ui| {
                ui.label("Write to:");
                ui.radio_value(&mut self.output, Output::Folder, "new folder (same structure)");
                ui.radio_value(&mut self.output, Output::InPlace, "in place (originals kept as .bak)");
            });
            if self.output == Output::Folder {
                ui.horizontal(|ui| {
                    ui.label("Folder:");
                    ui.add(egui::TextEdit::singleline(&mut self.out_dir).desired_width(ui.available_width() - 90.0));
                    if ui.button("Browse…").clicked()
                        && let Some(p) = rfd::FileDialog::new().pick_folder()
                    {
                        self.out_dir = p.display().to_string();
                    }
                });
            }
            ui.horizontal(|ui| {
                let can = !self.to_ai.is_empty() && (self.output == Output::InPlace || !self.out_dir.is_empty());
                if ui.add_enabled(can, egui::Button::new("Apply to all loaded copies…")).clicked() {
                    self.confirm = true;
                }
                if self.to_ai.is_empty() {
                    ui.label("Tick the players to hand to the AI.");
                }
            });
        }
        for f in add_files {
            let sess = self.session.as_mut().unwrap();
            match sess.add_file(&f) {
                Ok(msg) => self.log.push(format!("{}: {msg}", f.display())),
                Err(e) => self.log.push(format!("error: {e:#}")),
            }
        }
        if back {
            self.back_to_list();
        }
    }

    fn confirm_window(&mut self, ctx: &egui::Context) {
        if !self.confirm {
            return;
        }
        let sess = self.session.as_ref().unwrap();
        let seats = sess.seats();
        let names: Vec<String> = seats.iter().filter(|s| self.to_ai.contains(&s.id)).map(|s| s.name.clone()).collect();
        let missing: Vec<String> = seats
            .iter()
            .filter(|s| s.copy.is_none() && !self.to_ai.contains(&s.id))
            .map(|s| s.name.clone())
            .collect();
        let files: usize = sess.all_copies().map(|c| 1 + c.srv.is_some() as usize).sum();
        let mut go = None;
        egui::Window::new("Apply changes").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
            ui.label(format!("Hand to the AI: {}", names.join(", ")));
            ui.label(format!("Files to write: {files}"));
            ui.label(match self.output {
                Output::InPlace => "Edited in place; originals are kept as .bak.".to_string(),
                Output::Folder => format!("Written to {}", self.out_dir),
            });
            if !missing.is_empty() {
                ui.add_space(4.0);
                ui.colored_label(
                    RED,
                    format!(
                        "Copies of {} are not loaded. Their files will not be edited; apply the same change to them later, or they cannot rejoin this save.",
                        missing.join(", ")
                    ),
                );
            }
            ui.horizontal(|ui| {
                if ui.button("Apply").clicked() {
                    go = Some(true);
                }
                if ui.button("Cancel").clicked() {
                    go = Some(false);
                }
            });
        });
        match go {
            Some(true) => {
                self.confirm = false;
                let ids: Vec<u16> = self.to_ai.iter().copied().collect();
                let out = (self.output == Output::Folder).then(|| PathBuf::from(&self.out_dir));
                let sess = self.session.as_mut().unwrap();
                match sess.apply(&ids, self.keep_notices, out.as_deref()) {
                    Ok(l) => {
                        self.log.extend(l);
                        self.log.push("done".into());
                        self.to_ai.clear();
                    }
                    Err(e) => self.log.push(format!("error: {e:#} (nothing was written)")),
                }
            }
            Some(false) => self.confirm = false,
            None => {}
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, root: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = root.ctx().clone();
        self.poll_scan();
        egui::Panel::top("top").show(root, |ui| {
            ui.add_space(4.0);
            self.top_bar(ui);
            ui.add_space(4.0);
        });
        egui::Panel::bottom("log").resizable(true).default_size(130.0).show(root, |ui| {
            ui.horizontal(|ui| {
                ui.strong("Log");
                if ui.small_button("clear").clicked() {
                    self.log.clear();
                }
            });
            egui::ScrollArea::vertical().stick_to_bottom(true).auto_shrink([false, false]).show(ui, |ui| {
                for l in &self.log {
                    if l.starts_with("error") || l.starts_with("warning") {
                        ui.colored_label(RED, l);
                    } else {
                        ui.monospace(l);
                    }
                }
            });
        });
        egui::CentralPanel::default().show(root, |ui| {
            if self.session.is_some() {
                self.session_view(ui);
            } else {
                self.list_view(ui);
            }
        });
        self.confirm_window(&ctx);
    }
}
