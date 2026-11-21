# Handoff: guild-save-edit

State as of 2026-11-22, for an agent continuing this work.

## What this is

A Rust tool (CLI + egui GUI) to inspect and edit savegames of *Die Gilde Gold*
(The Guild Gold, German 2.05, save version `0x1005a`). The user plays network
games with friends and wants to continue them when a player drops out:

- hand a human player's dynasty to the AI;
- do that consistently in every player's copy of the save;
- generate the copies of players whose files are missing.

They are now broadening it into a general save explorer/editor, starting with a
human-readable view of every field.

## Locations

| Path | What |
|---|---|
| `/home/maxm/repos/guild/guild-save-edit` | this repo (GitHub: `git@github.com:Meidimax99/guild-save-editor.git`) |
| `/home/maxm/repos/guild/savegame/` | the user's collected saves (**game data, never commit**). `GILDE_SAVEGAME_2.*` at top level is Amadeus Abdomen's copy; `gilde/<friend>/` hold five other players' copies of the same save point; `network.zip` = the top-level files again |
| `/home/maxm/repos/guild/The Guild Gold (German)/` | game installation (auto-detected; text tables are read from it at runtime) |
| `/home/maxm/repos/guild/gilde-rs` | separate engine-reimplementation project. `src/{world,scene,reader,vfs,res,text}.rs` here are copies of its `gilde-formats` modules (adapted). The user does **not** want save editing integrated into gilde-rs |
| `/home/maxm/repos/guild/re-out/` | headless Ghidra project + full decompile (`decomp.c`, `functions.tsv`, `strings.tsv`), helpers `bin/fn <addr…>` (print functions), `bin/dec`, `bin/reexport`. Not in git (derived from the copyrighted exe) |
| `/home/maxm/repos/guild/gilde-rs/re/names.tsv` | identified function/global names (e.g. `sv_LoadWorld 0x50abf0`, `g_text_table 0x144a280`) |

## Branches

- `master` (pushed): CLI + GUI hand-over tool. Last commit `5b8b32c` (the
  family-link fix) is **not pushed yet**; ask before pushing.
- `explorer` (local only, current): `master` + Explorer tab + field names +
  value decoding + generating missing copies. Not merged into `master` yet;
  ask the user before merging or pushing.

## Build, run, test

```sh
cd /home/maxm/repos/guild/guild-save-edit
cargo build --release
./target/release/guild-save-edit-gui ../savegame            # scan folder (Players tab)
./target/release/guild-save-edit-gui ../savegame/GILDE_SAVEGAME_2.SAV   # opens Explorer
cargo test --release      # tests use ../savegame and the game dir if present, else skip
```

Rust via rustup in `~/.cargo/bin` (put it on `PATH`). Edition 2024; egui/eframe
0.37 (new API: `eframe::App::ui(&mut self, ui, frame)`, `egui::Panel::top/left/bottom`
instead of `TopBottomPanel`/`SidePanel`).

## Code map

| File | Role |
|---|---|
| `src/world.rs` | world-state parser (header, globals, objects, buildings, dynasties, characters, markets/cities, offices); records file offsets needed for patching (`kind_offset`, `f5c_offset`, `g71da90_offset`) |
| `src/scene.rs` | embedded `.ed3` scene; only its length matters (`Scene::parse_prefix`) |
| `src/save.rs` | `SaveFile`: gunzip, parse, patch (`set_kind`, `make_owner`, header human count), notices walker over dynamic sections 1–5, verified gzip write |
| `src/ops.rs` | edits shared by CLI/GUI: `to_ai`, `to_human`, `set_kind`, kinds/labels, `write_with_backup` |
| `src/session.rs` | network sessions: scan a tree, group copies by `SaveKey` (version, city, date, header checksum list), owner detection, twins/conflicts, `generate_missing`, `apply` (in place or `<out>/<player>/`) |
| `src/explore.rs` | generic tree (`serde_json::Value`) of everything parsed; hiding of technical fields; `label()` (field names); `Context` + `display()` (decoded values) |
| `src/{vfs,res,text}.rs` | game archives and global text table (for enum texts) |
| `src/main.rs` | CLI (`info`, `players`, `to-ai`, `to-human`, `set-kind`, `scan`, `apply`, `fields`, …) |
| `src/bin/guild-save-edit-gui/` | `main.rs` (tabs), `editor.rs` (Players / AI hand-over), `explorer.rs` (Explorer) |
| `docs/savegame-format.md` | the format and network findings (keep it current) |

## Key facts (details in docs/savegame-format.md)

- `.SAV`/`.SRV` are gzip. `.SRV` = prefix of `.SAV` (world + scene); `.SAV` adds ~8.4 MB dynamic state.
- Character kind `+0x02`: 5 AI dynasty, 6 human on the machine that saved this copy, 7 human on another machine.
- Every machine writes its own copy in `gamedata/network/`. Copies differ in
  owner identity, machine-local data (own notices, network command log) and
  small simulation noise. The header checksum list is identical in all of
  them.
- Rejoining requires the player's own local copy; the local player = the kind-6 character.
- Header `0x2e` = human count (the host waits for that many); keep it in sync.
- Character `+0x5c` = family links (spouse, children). Never reset them.
- Text ids for codes: gender 1161, religion 1158, rank (`+0x0d`) 276, master grade (`+0x164`) 304, job (`+0x165`) 525 (text table `0x144a280`, see `FUN_004d3ab0`).
- Building `+0x27` = owner character slot.

## Verified vs not

- **Verified:**
  - all parsers on the six real copies;
  - hand-over changes only the intended bytes;
  - a generated copy's header, kinds and local slot match the real copy;
  - generate → apply → rescan gives a complete save;
  - decoding of the fields listed above.
- **Not verified in the game:** loading an edited or a generated copy in an
  actual network game. The user would have to test that.
- **Dynamic sections 6–17:** the layouts are known only partially. Parsing
  them as documented ends 15734 bytes short of the end of the sample saves.

## Open work / likely next steps

1. User's latest complaint: many fields are still not human-interpretable. Candidates:
   - character floats `f10`–`f20` (probably position) and timers
     (`f17c`, `f184b`, `f18c`, `f19c`);
   - the money field (not yet located);
   - market wares (`f00` is probably a ware/prototype id; names would come
     from the game's prototype data, see gilde-rs);
   - objects (`f06`/`f0a` look like building/owner refs, inconsistent);
   - cities and offices.
   Method: find the code that reads the field (`grep` in `re-out/decomp.c`
   for the memory address, e.g. characters at `0x79cd40 + offset`), look for
   UI/text/debug strings, and confirm against the six copies.
2. Editing in the Explorer (not started): the view is read-only. Editing needs
   offsets per field (only a few are recorded) and a writer.
3. Ask whether to merge `explorer` into `master` and push.
4. Optional: decode the dynamic state beyond notices in the Explorer.

## User preferences / conventions

- Focus is the GUI; they said they don't care about CLI features for now.
- Docs (README, `docs/`) describe the final product only, with no development history.
- Don't commit game data, saves or decompiled code. Copyrighted game texts
  are loaded at runtime, not embedded.
- Commits end with the attribution line the harness provides; push only when asked.
- When in doubt about scope, implement the obvious part and report briefly;
  the user answers in short messages.

## Gotchas

- `pkill -f guild-save-edit-gui` inside a shell command whose text contains
  that name kills the shell itself (exit 144). Use
  `pgrep -f 'release/guild-save-edit-gui' | xargs -r kill` as a separate
  command, then start the GUI in another command with `setsid … &`.
- Screenshots: no xdotool/xwininfo. Get the window id via
  `i3-msg -t get_tree` (window name "Gilde Savegame Editor") and run
  `maim -i <id> file.png`.
- Ghidra decompile output for some functions has broken argument lists
  (stack confusion); cross-check with `strings.tsv` xrefs.
- The city-name header field (`0x05`, 32 bytes) contains leftover memory
  after the NUL; ignore it when comparing copies.
