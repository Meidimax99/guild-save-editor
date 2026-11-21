//! GUI for `guild-save-edit`.
//!
//! Views:
//! * **Players / AI hand-over**: scan the players' network saves, group the
//!   copies of a save point and hand players to the AI in all of them.
//! * **Explorer**: browse every parsed field of a single save file.
//!
//! Usage: `guild-save-edit-gui [folder | file]`. A folder is scanned in the
//! hand-over view; a file is opened in both views.

mod editor;
mod explorer;

use eframe::egui;
use std::path::PathBuf;

#[derive(PartialEq, Clone, Copy)]
enum Tab {
    Players,
    Explorer,
}

struct Shell {
    tab: Tab,
    editor: editor::App,
    explorer: explorer::Explorer,
}

impl eframe::App for Shell {
    fn ui(&mut self, root: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::Panel::top("tabs").show(root, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.tab, Tab::Players, egui::RichText::new("Players / AI hand-over").heading());
                ui.selectable_value(&mut self.tab, Tab::Explorer, egui::RichText::new("Explorer").heading());
            });
            ui.add_space(2.0);
        });
        match self.tab {
            Tab::Players => self.editor.show(root),
            Tab::Explorer => self.explorer.show(root),
        }
    }
}

fn main() -> eframe::Result {
    let initial = std::env::args().nth(1).map(PathBuf::from);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1200.0, 760.0]).with_title("Gilde Savegame Editor"),
        ..Default::default()
    };
    eframe::run_native(
        "guild-save-edit",
        options,
        Box::new(move |_cc| {
            let mut shell = Shell { tab: Tab::Players, editor: editor::App::default(), explorer: explorer::Explorer::default() };
            if let Some(p) = initial {
                shell.editor.root_input = p.display().to_string();
                if p.is_dir() {
                    shell.editor.start_scan(p);
                } else {
                    shell.editor.open_single(&p);
                    shell.explorer.open(p);
                    shell.tab = Tab::Explorer;
                }
            }
            Ok(Box::new(shell))
        }),
    )
}
