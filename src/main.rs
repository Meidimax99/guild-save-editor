//! `guild-save-edit`: inspect and edit savegames of *Die Gilde Gold*
//! (The Guild Gold, v2.05, save version 0x1005a).

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use guild_save_edit::ops::{self, dynasty_name, full_name, is_player, kind_name};
use guild_save_edit::save::{SaveFile, companions};
use guild_save_edit::session;
use std::path::{Path, PathBuf};


#[derive(Parser)]
#[command(version, about = "Inspect and edit Die Gilde Gold savegames (.SAV/.SRV)")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(clap::Args)]
struct Output {
    /// Write the result here (for a .SAV, the matching .SRV is written next to it).
    #[arg(short, long, conflicts_with = "in_place")]
    output: Option<PathBuf>,
    /// Overwrite the input file(s); the originals are kept as `<name>.bak`.
    #[arg(long)]
    in_place: bool,
    /// Only edit the given file, not its .SAV/.SRV companion.
    #[arg(long)]
    single: bool,
}

#[derive(Subcommand)]
enum Cmd {
    /// Summary of a savegame.
    Info { save: PathBuf },
    /// List the players (dynasty heads): human and AI.
    Players {
        save: PathBuf,
        /// Also list every character with its kind.
        #[arg(long)]
        all: bool,
    },
    /// Show one character's decoded fields.
    Character { save: PathBuf, id: u16 },
    /// List pending notices / message boxes in a .SAV.
    Notices {
        save: PathBuf,
        /// Only notices of this character.
        #[arg(long)]
        character: Option<u16>,
    },
    /// Hand a human player's character over to the computer.
    ToAi {
        save: PathBuf,
        /// Character id (see `players`).
        id: u16,
        /// Keep the player's pending interactive notices instead of cancelling
        /// them like the game does when a player leaves.
        #[arg(long)]
        keep_notices: bool,
        #[command(flatten)]
        out: Output,
    },
    /// Give an AI player's character to a human.
    ToHuman {
        save: PathBuf,
        id: u16,
        #[command(flatten)]
        out: Output,
    },
    /// Scan a folder tree for network saves and group every player's copy
    /// of the same save point.
    Scan { root: PathBuf },
    /// Hand players to the AI in every player's copy of one save point.
    Apply {
        /// Folder tree containing the players' saves.
        root: PathBuf,
        /// Which save point (number from `scan`, 1 = newest).
        #[arg(long, default_value_t = 1)]
        save: usize,
        /// Character ids to hand to the AI.
        #[arg(long = "ai", required = true, num_args = 1..)]
        ai: Vec<u16>,
        /// Additional single save files to include.
        #[arg(long = "add")]
        add: Vec<PathBuf>,
        #[arg(long)]
        keep_notices: bool,
        /// Write into this folder, mirroring the tree (default: in place with .bak).
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Set a character's raw kind byte (expert use).
    SetKind {
        save: PathBuf,
        id: u16,
        kind: u8,
        #[command(flatten)]
        out: Output,
    },
}

fn print_header(s: &SaveFile) {
    let h = &s.world.header;
    let d = &s.world.globals.date;
    let round = i32::from_le_bytes(d[0..4].try_into().unwrap());
    let hour = u16::from_le_bytes(d[4..6].try_into().unwrap());
    let minute = i32::from_le_bytes(d[6..10].try_into().unwrap());
    println!("file      {}", s.path.display());
    println!("kind      {}", if s.is_snapshot() { "snapshot (.SRV)" } else { "full save (.SAV)" });
    println!("version   {:#x}", h.version);
    println!("name      {}", h.description);
    println!("city      {}", h.name);
    println!("date      round {round} (year {}), {hour:02}:{minute:02}", 1400 + round);
}

fn cmd_info(path: &Path) -> Result<()> {
    let s = SaveFile::load(path)?;
    print_header(&s);
    let w = &s.world;
    println!(
        "contents  {} characters, {} buildings, {} objects, cities {}",
        w.characters.list.len(),
        w.buildings.len(),
        w.objects.len(),
        w.cities.iter().map(|c| c.name.as_str()).collect::<Vec<_>>().join(", ")
    );
    println!(
        "human players: {} recorded in header, {} characters marked human",
        s.header_human_count(),
        ops::human_count(&s)
    );
    if !s.is_snapshot() {
        match s.notices() {
            Ok(n) => println!("pending notices: {}", n.notices.len()),
            Err(e) => println!("dynamic state: {e:#}"),
        }
    }
    println!();
    print_players(&s, false);
    Ok(())
}

fn print_players(s: &SaveFile, all: bool) {
    println!("{:>5}  {:<24} {:<26} {:<16} kind", "id", "role", "name", "dynasty");
    for c in s.world.characters.list.iter().filter(|c| all || is_player(c)) {
        let local = "";
        println!(
            "{:>5}  {:<24} {:<26} {:<16} {}{local}",
            c.id,
            kind_name(c.f02),
            full_name(c),
            dynasty_name(s, c),
            c.f02
        );
    }
}

fn cmd_notices(path: &Path, who: Option<u16>) -> Result<()> {
    let s = SaveFile::load(path)?;
    let sec = s.notices()?;
    println!("{:>4} {:>6} {:>5}  text", "#", "kind", "char");
    for (i, n) in sec.notices.iter().enumerate().filter(|(_, n)| who.is_none_or(|w| n.character == w)) {
        println!("{i:>4} {:#6x} {:>5}  {}", n.kind, n.character, n.text.replace('\n', " "));
    }
    Ok(())
}

/// Resolve the files to edit and where each one goes.
fn targets(path: &Path, out: &Output) -> Result<Vec<(PathBuf, PathBuf)>> {
    let files = if out.single { vec![path.to_owned()] } else { companions(path) };
    if out.in_place {
        return Ok(files.into_iter().map(|f| (f.clone(), f)).collect());
    }
    let Some(o) = &out.output else { bail!("give --output <file> or --in-place") };
    Ok(files
        .into_iter()
        .map(|f| {
            let ext = f.extension().map(|e| e.to_owned()).unwrap_or_default();
            let dst = if f == path { o.clone() } else { o.with_extension(ext) };
            (f, dst)
        })
        .collect())
}

fn edit(path: &Path, out: &Output, mut f: impl FnMut(&mut SaveFile) -> Result<Vec<String>>) -> Result<()> {
    let plan = targets(path, out)?;
    let mut done = Vec::new();
    for (src, dst) in &plan {
        let mut s = SaveFile::load(src)?;
        for line in f(&mut s).with_context(|| format!("editing {}", src.display()))? {
            println!("{line}");
        }
        done.push((s, dst.clone()));
    }
    // Write only after every file was edited successfully.
    for (s, dst) in &done {
        for line in ops::write_with_backup(s, dst)? {
            println!("{line}");
        }
    }
    Ok(())
}

fn print_session(sess: &session::Session) {
    println!("save      {} — {} — {}", sess.name(), sess.key.city, session::date_text(sess.reference()));
    for seat in sess.seats() {
        let file = match seat.copy {
            Some(i) => sess.copies[i].sav.path.display().to_string(),
            None => "MISSING".into(),
        };
        println!("  {:>4}  {:<26} {:<16} {:<26} {}", seat.id, seat.name, seat.dynasty, session::seat_role(seat.kind), file);
    }
    for c in &sess.copies {
        for n in &c.notes {
            println!("  note: {}: {n}", c.sav.path.display());
        }
    }
    for c in &sess.copies {
        for t in &c.twins {
            println!("  identical copy (edited too): {}", t.sav.path.display());
        }
    }
    for (p, why) in &sess.conflicts {
        println!("  NOT USED: {why}");
        let _ = p;
    }
}

fn scan(root: &Path) -> Result<session::ScanResult> {
    let r = session::scan(root, |_, _| {});
    for e in &r.errors {
        eprintln!("skipped {e}");
    }
    Ok(r)
}

fn main() -> Result<()> {
    match Cli::parse().cmd {
        Cmd::Info { save } => cmd_info(&save),
        Cmd::Players { save, all } => {
            let s = SaveFile::load(&save)?;
            print_players(&s, all);
            Ok(())
        }
        Cmd::Character { save, id } => {
            let s = SaveFile::load(&save)?;
            let c = s.character(id)?;
            println!("{} — {} ({})\n{c:#?}", full_name(c), kind_name(c.f02), dynasty_name(&s, c));
            Ok(())
        }
        Cmd::Notices { save, character } => cmd_notices(&save, character),
        Cmd::ToAi { save, id, keep_notices, out } => edit(&save, &out, |s| ops::to_ai(s, id, keep_notices)),
        Cmd::ToHuman { save, id, out } => edit(&save, &out, |s| ops::to_human(s, id)),
        Cmd::Scan { root } => {
            let r = scan(&root)?;
            for (i, f) in r.found.into_iter().enumerate() {
                println!("[{}] newest file {}, {} cop{}", i + 1, session::time_text(f.newest()), f.copies.len(), if f.copies.len() == 1 { "y" } else { "ies" });
                print_session(&session::Session::new(f, Some(root.clone())));
                println!();
            }
            Ok(())
        }
        Cmd::Apply { root, save, ai, add, keep_notices, output } => {
            let r = scan(&root)?;
            let Some(f) = r.found.into_iter().nth(save.saturating_sub(1)) else { bail!("no save point #{save}") };
            let mut sess = session::Session::new(f, Some(root.clone()));
            for p in &add {
                println!("{}", sess.add_file(p)?);
            }
            print_session(&sess);
            for line in sess.apply(&ai, keep_notices, output.as_deref())? {
                println!("{line}");
            }
            Ok(())
        }
        Cmd::SetKind { save, id, kind, out } => edit(&save, &out, |s| ops::set_kind(s, id, kind, None)),
    }
}
