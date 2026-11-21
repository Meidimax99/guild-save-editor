# guild-save-edit

A small standalone tool to inspect and edit savegames of *Die Gilde Gold*
(The Guild Gold, version 2.05, save format `0x1005a`).

Its main purpose: **hand a human player's dynasty to the computer**, so a
multiplayer game can go on after someone can no longer play. It can also do
the reverse and hand an AI dynasty to a human.

## Build

```sh
cargo build --release
# command line: target/release/guild-save-edit
# GUI:          target/release/guild-save-edit-gui [save.SAV]
```

## GUI

`guild-save-edit-gui` opens a save pair (Browse…, or type a path and press
Enter), shows the save's date and its players, and offers **Hand to AI** for
human players and **Make human** for AI players. Changes stay in memory until
you click **Save** (overwrites; originals kept as `.bak`) or **Save as…** (a
new `.SAV` with its `.SRV` next to it). Every action and any error shows in
the log at the bottom. It applies the same edits as the command line.

## Usage

Saves are in `Resources/gamedata/saves/` inside the game folder. Each save
is a pair: `NAME.SAV` (the full state) and `NAME.SRV` (a snapshot of the
world and city). Editing commands change **both files** unless you pass
`--single`.

```sh
# overview and player list
guild-save-edit info    saves/GILDE_SAVEGAME_2.SAV
guild-save-edit players saves/GILDE_SAVEGAME_2.SAV

#    id  role                     name                       dynasty          kind
#     1  human player             Amadeus Abdomen            Abdomen          6  (local)
#     3  human player             Pastorius Pinkelmann       Pinkelmann       6
#     2  AI player                Ana Aalschleck             Aalschleck       7
#   ...

# hand player 3 to the AI, writing a new save pair (MYGAME.SAV + MYGAME.SRV)
guild-save-edit to-ai saves/GILDE_SAVEGAME_2.SAV 3 -o saves/MYGAME.SAV

# or edit in place (originals are kept as *.SAV.bak / *.SRV.bak)
guild-save-edit to-ai saves/GILDE_SAVEGAME_2.SAV 3 --in-place

# the reverse: let a human take over AI dynasty 4
guild-save-edit to-human saves/GILDE_SAVEGAME_2.SAV 4 -o saves/MYGAME.SAV
```

Other commands: `character <save> <id>` (decoded fields of one character),
`notices <save> [--character id]` (pending message boxes), and
`set-kind <save> <id> <kind>` (write the raw kind byte; for experts).

Every written file is decompressed and parsed again before it replaces
anything. If an edit fails for either file of the pair, neither is written.

## What `to-ai` changes

This mirrors what the game itself does when a human player drops out
(`FUN_00527c60` in `GildeGold_TL.exe`):

* The character's kind byte (`+0x02`) goes from `6` (human player) to `5`
  (AI player that took over). Original AI dynasties use `7`.
* The eight per-character action slots at `+0x5c` are reset to `-1`.
* Pending notices of that character which need a human decision are removed
  from the `.SAV` (debt warnings, offers, and so on: kinds `0x11 0x2d 0x2e
  0x30 0x35 0x41 0x42 0x46 0x4b 0x5a 0x5b 0x5c 0x5d 0x60`). Pass
  `--keep-notices` to keep them.

Unlike the game, the tool does **not** give the character a new random first
name or a new dynasty id. The dynasty keeps its name, family, buildings,
and money.

## Caveats

* Only tested by parsing the result again. Load the edited save in the game
  before relying on it.
* The save stores which character is the *local* player (`(local)` in the
  listing; usually the host). Don't convert that one unless you mean to: if
  no human player is left, the game may end.
* In a multiplayer game, the host loads the edited save and the others join
  as usual.

## Format notes

* `.SAV`/`.SRV`: gzip stream. Inside is the world-state format of the `.cty`
  city files (header flags `5` = save, `6` = snapshot), then the embedded
  `.ed3` city scene, then (in `.SAV` only) about 8 MB of dynamic simulation
  state.
* Characters are 0x21c-byte records in memory (array at `0x79cd40`). Kind
  names come from the game's debug table at `0x637d70`: `0` DUMMY_NPC,
  `1` MEISTER_NPC, `2` POLITICIAN_NPC, `5` PLAYER, `6` REMOTE_PLAYER,
  `7` RELATIVE_PLAYER, `8` USED_PLAYER, `9` INVISIBLE_NPC, plus `15` = dead
  (`gm_KillSpieler`). Game logic treats `6` as human (round processing and
  game over when no living `6` is left), and `5`/`7` as computer dynasties.
* The parsers in `src/world.rs`, `src/scene.rs` and `src/reader.rs` are
  copied from the `gilde-rs` reimplementation so this tool stands alone.
