//! Uses the collected saves in ../savegame when present (not in the repo).
use guild_save_edit::save::SaveFile;
use std::path::Path;

#[test]
fn copy_for_another_player_matches_their_header() {
    let src = Path::new("../savegame/GILDE_SAVEGAME_2.SAV");
    let real = Path::new("../savegame/gilde/Josias/GILDE_SAVEGAME_2.SAV");
    if !src.exists() || !real.exists() {
        eprintln!("sample saves not present; skipped");
        return;
    }
    let mut s = SaveFile::load(src).unwrap();
    let r = SaveFile::load(real).unwrap();
    let target = r.world.characters.list.iter().find(|c| c.f02 == 6).unwrap().id;
    s.make_owner(target).unwrap();
    // Header up to the checksum list equals the real copy, except wealth
    // (0x65..0x69) and leftover memory after the city name's terminator.
    assert_eq!(s.raw[..0x0f], r.raw[..0x0f]);
    assert_eq!(s.raw[0x25..0x65], r.raw[0x25..0x65]);
    assert_eq!(s.raw[0x69..0x93], r.raw[0x69..0x93]);
    // Same owner, same human kinds.
    let kinds = |f: &SaveFile| f.world.characters.list.iter().map(|c| (c.id, c.f02)).filter(|(_, k)| matches!(k, 5 | 6 | 7)).collect::<Vec<_>>();
    assert_eq!(kinds(&s), kinds(&r));
    assert_eq!(s.world.characters.g71da90, r.world.characters.g71da90);
}
