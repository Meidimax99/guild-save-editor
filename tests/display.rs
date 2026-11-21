//! Uses the collected saves in ../savegame and the game installation next to
//! the repo when present (neither is in the repo).
use guild_save_edit::explore::{self, Step};
use std::path::Path;

#[test]
fn decoded_values() {
    let p = Path::new("../savegame/GILDE_SAVEGAME_2.SAV");
    if !p.exists() {
        eprintln!("sample save not present; skipped");
        return;
    }
    let d = explore::load(p).unwrap();
    let path = [Step::Key("characters".into()), Step::Key("list".into()), Step::Index(1)];
    let ch = explore::get(&d.root, &path).unwrap();
    let show = |k: &str| explore::display(&d.ctx, "characters/list", k, &ch[k]).unwrap_or_default();
    for k in ["f02", "f09", "f0c", "f0d", "f164", "f50", "f54", "f5c", "f10", "f1c"] {
        println!("{k:6} {}", show(k));
    }
    assert!(show("f5c").starts_with("spouse: "), "{}", show("f5c"));
    assert!(show("f50").starts_with("Abdomen"), "{}", show("f50"));
    if d.ctx.texts.is_some() {
        assert_eq!(show("f09"), "männlich (0)");
        assert_eq!(show("f0d"), "Bürger (2)");
    } else {
        eprintln!("game texts not found; enum texts not checked");
    }
    let date = explore::display(&d.ctx, "globals", "date", &d.root["globals"]["date"]).unwrap();
    println!("date   {date}");
    assert!(date.starts_with("1419 06:58"));
}
