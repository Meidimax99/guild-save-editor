//! GUI for `guild-save-edit`: open a savegame pair, switch players between
//! human and AI, save.

use eframe::egui;
use guild_save_edit::ops::{self, KIND_AI, KIND_HUMAN_LOCAL, KIND_HUMAN_REMOTE};
use guild_save_edit::save::{SaveFile, companions};
use std::path::{Path, PathBuf};

fn main() -> eframe::Result {
    let initial = std::env::args().nth(1).map(PathBuf::from);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([900.0, 620.0]).with_title("Gilde Savegame Editor"),
        ..Default::default()
    };
    eframe::run_native(
        "guild-save-edit",
        options,
        Box::new(move |_cc| {
            let mut app = App::default();
            app.path_input = initial.as_ref().map(|p| p.display().to_string()).unwrap_or_else(default_dir);
            if let Some(p) = initial {
                app.open(&p);
            }
            Ok(Box::new(app))
        }),
    )
}

/// The game's save folder if it can be found next to this repo.
fn default_dir() -> String {
    for c in ["../The Guild Gold (German)/Resources/gamedata/saves", "../savegame"] {
        if let Ok(p) = std::fs::canonicalize(c) {
            return p.display().to_string();
        }
    }
    String::new()
}

#[derive(Default)]
struct App {
    path_input: String,
    /// The loaded `.SAV`/`.SRV` pair (edits are applied to all of them).
    files: Vec<SaveFile>,
    dirty: bool,
    keep_notices: bool,
    show_all: bool,
    log: Vec<String>,
    confirm_last_human: Option<u16>,
}

impl App {
    fn open(&mut self, path: &Path) {
        let mut files = Vec::new();
        for p in companions(path) {
            match SaveFile::load(&p) {
                Ok(s) => files.push(s),
                Err(e) => {
                    self.log.push(format!("error: {e:#}"));
                    return;
                }
            }
        }
        // Show the full save (.SAV) first.
        files.sort_by_key(|s| s.is_snapshot());
        self.log.push(format!("opened {}", files.iter().map(|f| f.path.display().to_string()).collect::<Vec<_>>().join(" + ")));
        self.path_input = path.display().to_string();
        self.files = files;
        self.dirty = false;
    }

    /// Apply an edit to every file of the pair; roll back all on failure.
    fn apply(&mut self, f: impl Fn(&mut SaveFile) -> anyhow::Result<Vec<String>>) {
        let backup: Vec<Vec<u8>> = self.files.iter().map(|s| s.raw.clone()).collect();
        let mut log = Vec::new();
        for s in &mut self.files {
            match f(s) {
                Ok(l) => log.extend(l),
                Err(e) => {
                    for (s, raw) in self.files.iter_mut().zip(backup) {
                        s.raw = raw;
                        let _ = s.reparse();
                    }
                    self.log.push(format!("error: {e:#}"));
                    return;
                }
            }
        }
        self.log.extend(log);
        self.dirty = true;
    }

    fn save_to(&mut self, main_dst: Option<PathBuf>) {
        let Some(first) = self.files.first() else { return };
        let main_src = first.path.clone();
        let mut log = Vec::new();
        for s in &self.files {
            let dst = match &main_dst {
                None => s.path.clone(),
                Some(d) if s.path == main_src => d.clone(),
                Some(d) => d.with_extension(s.path.extension().unwrap_or_default()),
            };
            match ops::write_with_backup(s, &dst) {
                Ok(l) => log.extend(l),
                Err(e) => {
                    log.push(format!("error: {e:#}"));
                    self.log.extend(log);
                    return;
                }
            }
        }
        self.log.extend(log);
        self.dirty = false;
        if let Some(d) = main_dst {
            self.open(&d);
        }
    }

    fn toolbar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("Save file:");
            let w = ui.available_width() - 260.0;
            let resp = ui.add(egui::TextEdit::singleline(&mut self.path_input).desired_width(w.max(200.0)));
            if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                let p = PathBuf::from(&self.path_input);
                self.open(&p);
            }
            if ui.button("Browse…").clicked() {
                let mut dlg = rfd::FileDialog::new().add_filter("Gilde savegame", &["SAV", "sav", "SRV", "srv"]);
                let dir = PathBuf::from(&self.path_input);
                let dir = if dir.is_dir() { Some(dir) } else { dir.parent().map(Path::to_path_buf) };
                if let Some(d) = dir {
                    dlg = dlg.set_directory(d);
                }
                if let Some(p) = dlg.pick_file() {
                    self.open(&p);
                }
            }
            if ui.button("Open").clicked() {
                let p = PathBuf::from(&self.path_input);
                self.open(&p);
            }
            ui.add_enabled_ui(!self.files.is_empty(), |ui| {
                if ui.button("Reload").on_hover_text("Discard changes and reload from disk").clicked() {
                    let p = self.files[0].path.clone();
                    self.open(&p);
                }
            });
        });
    }

    fn info(&self, ui: &mut egui::Ui) {
        let s = &self.files[0];
        let h = &s.world.header;
        let (round, year, hour, minute) = ops::date(s);
        egui::Grid::new("info").num_columns(2).spacing([16.0, 2.0]).show(ui, |ui| {
            ui.label("Name");
            ui.strong(&h.description);
            ui.end_row();
            ui.label("City");
            ui.label(&h.name);
            ui.end_row();
            ui.label("Date");
            ui.label(format!("{year} (round {round}), {hour:02}:{minute:02}"));
            ui.end_row();
            ui.label("Files");
            ui.label(self.files.iter().map(|f| f.path.file_name().unwrap_or_default().to_string_lossy()).collect::<Vec<_>>().join(" + "));
            ui.end_row();
            ui.label("Human players");
            ui.label(format!(
                "{} (header; the host waits for this many when loading)",
                s.header_human_count()
            ));
            ui.end_row();
            ui.label("Contents");
            ui.label(format!(
                "{} characters, {} buildings, version {:#x}",
                s.world.characters.list.len(),
                s.world.buildings.len(),
                h.version
            ));
            ui.end_row();
        });
    }

    fn players(&mut self, ui: &mut egui::Ui) {
        enum Action {
            ToAi(u16),
            ToHuman(u16),
        }
        let mut action = None;
        let s = &self.files[0];
        let owner_ids: Vec<u16> =
            s.world.characters.list.iter().filter(|c| c.f02 == KIND_HUMAN_LOCAL).map(|c| c.id).collect();
        let rows: Vec<_> = s.world.characters.list.iter().filter(|c| self.show_all || ops::is_player(c)).collect();
        egui::ScrollArea::vertical().max_height(ui.available_height() - 150.0).show(ui, |ui| {
            egui::Grid::new("players").striped(true).num_columns(6).spacing([14.0, 4.0]).show(ui, |ui| {
                for h in ["ID", "Name", "Dynasty", "Role", "", ""] {
                    ui.strong(h);
                }
                ui.end_row();
                for c in rows {
                    ui.label(c.id.to_string());
                    let name = ops::full_name(c);
                    ui.label(name);
                    ui.label(ops::dynasty_name(s, c));
                    let role = ops::kind_name(c.f02);
                    match c.f02 {
                        KIND_HUMAN_LOCAL | KIND_HUMAN_REMOTE => ui.colored_label(egui::Color32::from_rgb(90, 170, 255), role),
                        KIND_AI => ui.colored_label(egui::Color32::from_rgb(230, 170, 60), role),
                        _ => ui.label(format!("{role} ({})", c.f02)),
                    };
                    match c.f02 {
                        KIND_HUMAN_LOCAL | KIND_HUMAN_REMOTE => {
                            if ui.button("Hand to AI").clicked() {
                                action = Some(Action::ToAi(c.id));
                            }
                        }
                        KIND_AI => {
                            if ui.button("Make human").on_hover_text("Marks the dynasty as another human player's (kind 7)").clicked() {
                                action = Some(Action::ToHuman(c.id));
                            }
                        }
                        _ => {
                            ui.label("");
                        }
                    }
                    ui.label("");
                    ui.end_row();
                }
            });
        });
        let keep = self.keep_notices;
        match action {
            Some(Action::ToAi(id)) if owner_ids.contains(&id) => self.confirm_last_human = Some(id),
            Some(Action::ToAi(id)) => self.apply(|s| ops::to_ai(s, id, keep)),
            Some(Action::ToHuman(id)) => self.apply(|s| ops::to_human(s, id)),
            None => {}
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, root: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = root.ctx().clone();
        egui::Panel::top("top").show(root, |ui| {
            ui.add_space(4.0);
            self.toolbar(ui);
            ui.add_space(4.0);
        });
        egui::Panel::bottom("log").resizable(true).default_size(110.0).show(root, |ui| {
            ui.horizontal(|ui| {
                ui.strong("Log");
                if ui.small_button("clear").clicked() {
                    self.log.clear();
                }
            });
            egui::ScrollArea::vertical().stick_to_bottom(true).auto_shrink([false, false]).show(ui, |ui| {
                for l in &self.log {
                    if l.starts_with("error") || l.starts_with("warning") {
                        ui.colored_label(egui::Color32::from_rgb(230, 90, 80), l);
                    } else {
                        ui.monospace(l);
                    }
                }
            });
        });
        egui::CentralPanel::default().show(root, |ui| {
            if self.files.is_empty() {
                ui.centered_and_justified(|ui| {
                    ui.label("Open a savegame (.SAV) from Resources/gamedata/saves");
                });
                return;
            }
            self.info(ui);
            ui.separator();
            ui.horizontal(|ui| {
                ui.heading("Players");
                ui.checkbox(&mut self.show_all, "show all characters");
                ui.checkbox(&mut self.keep_notices, "keep pending notices when handing to AI")
                    .on_hover_text("By default, open message boxes that need a human decision are cancelled, like the game does when a player leaves");
            });
            self.players(ui);
            ui.separator();
            ui.horizontal(|ui| {
                ui.add_enabled_ui(self.dirty, |ui| {
                    if ui.button("💾 Save (overwrite, keeps .bak)").clicked() {
                        self.save_to(None);
                    }
                });
                if ui.button("Save as…").clicked() {
                    let p = &self.files[0].path;
                    let mut dlg = rfd::FileDialog::new().add_filter("Gilde savegame", &["SAV"]).set_file_name("GILDE_SAVEGAME_EDITED.SAV");
                    if let Some(d) = p.parent() {
                        dlg = dlg.set_directory(d);
                    }
                    if let Some(dst) = dlg.save_file() {
                        self.save_to(Some(dst));
                    }
                }
                if self.dirty {
                    ui.colored_label(egui::Color32::from_rgb(230, 170, 60), "unsaved changes");
                }
            });
        });

        if let Some(id) = self.confirm_last_human {
            egui::Window::new("Owner of this save").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(&ctx, |ui| {
                ui.label("In a network game every player has their own copy of the save, and this character is the player who owns this copy. If you hand it to the AI, whoever loads this file has no character of their own.");
                ui.horizontal(|ui| {
                    if ui.button("Hand to AI anyway").clicked() {
                        let keep = self.keep_notices;
                        self.apply(|s| ops::to_ai(s, id, keep));
                        self.confirm_last_human = None;
                    }
                    if ui.button("Cancel").clicked() {
                        self.confirm_last_human = None;
                    }
                });
            });
        }
    }
}
