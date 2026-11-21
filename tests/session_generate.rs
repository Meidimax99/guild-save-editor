//! Uses the collected saves in ../savegame when present (not in the repo).
use guild_save_edit::session::{self, Session};
use std::path::Path;

#[test]
fn generate_missing_copies_and_write_all() {
    let root = Path::new("../savegame");
    if !root.join("gilde").exists() {
        eprintln!("sample saves not present; skipped");
        return;
    }
    let scan = session::scan(root, |_, _| {});
    let found = scan.found.into_iter().max_by_key(|f| f.copies.len()).unwrap();
    let mut sess = Session::new(found, Some(root.to_path_buf()));
    let missing = sess.missing();
    assert!(!missing.is_empty(), "test expects at least one missing player");
    let src = sess.copies.iter().position(|c| c.generated_from.is_none()).unwrap();
    sess.generate_missing(src).unwrap();
    assert!(sess.missing().is_empty());

    // Hand one player (who has a real copy) to the AI and write everything.
    let ai = sess.copies.iter().rev().find(|c| c.generated_from.is_none()).unwrap().owner.unwrap();
    let out = Path::new(env!("CARGO_TARGET_TMPDIR")).join("generated-out");
    let _ = std::fs::remove_dir_all(&out);
    sess.apply(&[ai], false, Some(&out)).unwrap();

    // The output scans as one save with a copy for every remaining human.
    let rescan = session::scan(&out, |_, _| {});
    assert_eq!(rescan.found.len(), 1, "{:?}", rescan.errors);
    let s2 = Session::new(rescan.found.into_iter().next().unwrap(), Some(out.clone()));
    assert!(s2.missing().is_empty());
    for c in &s2.copies {
        // The handed-over player's own copy has no kind-6 character left.
        if c.owner != Some(ai) {
            assert!(c.notes.is_empty(), "{}: {:?}", c.sav.path.display(), c.notes);
        }
        assert!(c.srv.is_some());
        let r = c.sav.character(ai).unwrap();
        assert_eq!(r.f02, 5, "handed-over player is AI in {}", c.sav.path.display());
    }
    assert!(s2.seats().iter().all(|s| s.id != ai || !session::is_human_kind(s.kind)));
}
