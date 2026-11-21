//! Edits shared by the command line and the GUI.

use crate::reader::fixed_cstr;
use crate::save::{AI_CANCELLED_NOTICES, SaveFile};
use crate::world::Character;
use anyhow::{Result, bail};

/// Character kinds (`+0x02`) relevant for players.
///
/// Every machine in a network game saves its own copy. In that copy its own
/// character is `6` and the other humans are `7`; computer dynasties are `5`.
/// On load the local player is the character with kind `6`.
pub const KIND_AI: u8 = 5;
pub const KIND_HUMAN_LOCAL: u8 = 6;
pub const KIND_HUMAN_REMOTE: u8 = 7;

pub fn kind_name(k: u8) -> &'static str {
    match k {
        0 => "npc",
        1 => "master npc",
        2 => "politician npc",
        KIND_AI => "AI player",
        KIND_HUMAN_LOCAL => "human (owner of this file)",
        KIND_HUMAN_REMOTE => "human (other player)",
        8 => "used player",
        9 => "invisible npc",
        15 => "dead",
        _ => "?",
    }
}

pub fn is_player(c: &Character) -> bool {
    matches!(c.f02, KIND_AI | KIND_HUMAN_LOCAL | KIND_HUMAN_REMOTE)
}

pub fn full_name(c: &Character) -> String {
    format!("{} {}", fixed_cstr(&c.f30), fixed_cstr(&c.f40)).trim().to_owned()
}

pub fn dynasty_name(s: &SaveFile, c: &Character) -> String {
    s.world
        .dynasties
        .iter()
        .find(|d| d.id == c.f50)
        .map(|d| fixed_cstr(&d.f02))
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| format!("#{:#x}", c.f50))
}

/// Game date of the save as (round, year, hour, minute).
pub fn date(s: &SaveFile) -> (i32, i32, u16, i32) {
    let d = &s.world.globals.date;
    let round = i32::from_le_bytes(d[0..4].try_into().unwrap());
    let hour = u16::from_le_bytes(d[4..6].try_into().unwrap());
    let minute = i32::from_le_bytes(d[6..10].try_into().unwrap());
    (round, 1400 + round, hour, minute)
}

pub fn is_human(c: &Character) -> bool {
    matches!(c.f02, KIND_HUMAN_LOCAL | KIND_HUMAN_REMOTE)
}

pub fn human_count(s: &SaveFile) -> usize {
    s.world.characters.list.iter().filter(|c| is_human(c)).count()
}

/// Keep the header's human-player count in line with the characters.
fn sync_header_count(s: &mut SaveFile, log: &mut Vec<String>) {
    let old = s.header_human_count();
    let new = human_count(s).min(255) as u8;
    if old != new {
        s.set_header_human_count(new);
        log.push(format!("  human players in header: {old} -> {new}"));
    }
}

/// Hand a human player's character to the AI, as the game does when a player
/// leaves (`FUN_00527c60`). Returns log lines.
pub fn to_ai(s: &mut SaveFile, id: u16, keep_notices: bool) -> Result<Vec<String>> {
    let c = s.character(id)?;
    if !is_human(c) {
        bail!("character {id} ({}) is {}, not a human player", full_name(c), kind_name(c.f02));
    }
    let mut log = vec![format!("{}: {} ({}) -> AI", s.path.display(), full_name(c), dynasty_name(s, c))];
    if c.f02 == KIND_HUMAN_LOCAL {
        log.push(format!(
            "warning: {} owns this copy of the save; whoever loads it now has no character of their own",
            full_name(c)
        ));
    }
    s.set_kind(id, KIND_AI)?;
    if !s.is_snapshot() && !keep_notices {
        let sec = s.notices()?;
        let drop: Vec<usize> = sec
            .notices
            .iter()
            .enumerate()
            .filter(|(_, n)| n.character == id && AI_CANCELLED_NOTICES.contains(&n.kind))
            .map(|(i, _)| i)
            .collect();
        if !drop.is_empty() {
            log.push(format!("  cancelled {} pending notice(s) that need a human answer", drop.len()));
            s.drop_notices(&sec, &drop)?;
        }
    }
    s.reparse()?;
    sync_header_count(s, &mut log);
    s.reparse()?;
    Ok(log)
}

/// Set a character's kind, optionally requiring its current kind.
pub fn set_kind(s: &mut SaveFile, id: u16, kind: u8, expect: Option<&[u8]>) -> Result<Vec<String>> {
    let c = s.character(id)?;
    if let Some(ok) = expect
        && !ok.contains(&c.f02)
    {
        bail!("character {id} ({}) is {}, which cannot be changed this way", full_name(c), kind_name(c.f02));
    }
    let mut log = vec![format!("{}: {} kind {} -> {kind} ({})", s.path.display(), full_name(c), c.f02, kind_name(kind))];
    s.set_kind(id, kind)?;
    s.reparse()?;
    sync_header_count(s, &mut log);
    s.reparse()?;
    Ok(log)
}

pub fn to_human(s: &mut SaveFile, id: u16) -> Result<Vec<String>> {
    set_kind(s, id, KIND_HUMAN_REMOTE, Some(&[KIND_AI]))
}

/// Write a save, keeping `<name>.bak` when overwriting the original.
pub fn write_with_backup(s: &SaveFile, dst: &std::path::Path) -> Result<Vec<String>> {
    let mut log = Vec::new();
    if dst == s.path {
        let bak = std::path::PathBuf::from(format!("{}.bak", s.path.display()));
        if !bak.exists() {
            std::fs::copy(&s.path, &bak)?;
            log.push(format!("backup    {}", bak.display()));
        }
    }
    s.write(dst)?;
    log.push(format!("wrote     {}", dst.display()));
    Ok(log)
}
