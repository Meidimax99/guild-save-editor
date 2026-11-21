# guild-save-edit

A small standalone tool to inspect and edit savegames of *Die Gilde Gold*
(The Guild Gold, version 2.05, save format `0x1005a`).

Its main purpose: **hand a human player's dynasty to the computer**, so a
network game can go on after someone can no longer play. It can also do
the reverse and hand an AI dynasty to a human.

## Build

```sh
cargo build --release
# command line: target/release/guild-save-edit
# GUI:          target/release/guild-save-edit-gui [save.SAV]
```

## GUI: editing every player's copy at once

```sh
guild-save-edit-gui [folder-or-save]
```

1. **Scan** a folder holding the saves you collected from the players.
   Sub-folders are searched, e.g. one per player.
2. The tool groups all `.SAV` files by save point. Copies of the same save
   from different players count as one save. If several saves are found,
   it lists them with name, city, in-game date, the players found, and the
   newest file time.
3. **Open** one: all copies of it are loaded. For each human player the
   table shows that player's file, or **missing — not yet available**.
   - **Add save file(s)…** loads more copies, and **Add folder…** searches
     a folder and its sub-folders for them. The tool checks that each
     added file is a copy of this save and belongs to a player without one.
   - A byte-identical second copy of a player is edited along with it.
   - A *different* copy of the same player is reported and left alone.
   - Not every player's copy has to be present.
4. Tick **Hand to AI** for the players who left, then **Apply**. Every
   loaded copy (`.SAV` and `.SRV`) is edited the same way. The output goes
   either in place (originals kept as `.bak`) or into a new folder with one
   sub-folder per player: `<out>/<player name>/NAME.SAV` and `.SRV`, ready
   to send back to that player. Their game expects the files in
   `Resources/gamedata/network/`.

How a copy is identified:
- **Owner:** the character of kind `6` in it, cross-checked against the
  owner's name in the header.
- **Save point:** the header checksum list (`+0x78`) plus version, city,
  and in-game date. The lobby uses the same checksum list to match the
  players' copies, so it is identical in all copies of one save.

The command line has the same workflow:

```sh
guild-save-edit scan  collected/                          # list saves and copies
guild-save-edit apply collected/ --save 1 --ai 4 5 -o collected-edited/
guild-save-edit apply collected/ --save 1 --ai 4 --add extra/ANA.SAV   # in place
```

## Usage

Saves are in `Resources/gamedata/saves/` inside the game folder. Each save
is a pair: `NAME.SAV` (the full state) and `NAME.SRV` (a snapshot of the
world and city). Editing commands change **both files** unless you pass
`--single`.

```sh
# overview and player list
guild-save-edit info    saves/GILDE_SAVEGAME_2.SAV
guild-save-edit players saves/GILDE_SAVEGAME_2.SAV

#    id  role                       name                       dynasty      kind
#     1  human (owner of this file) Amadeus Abdomen            Abdomen      6
#     2  human (other player)       Ana Aalschleck             Aalschleck   7
#     3  human (other player)       Pastorius Pinkelmann       Pinkelmann   7
#    26  AI player                  Karola Nepper              #0x8007      5

# hand player 3 to the AI (do this in every remaining player's copy), writing a new pair (MYGAME.SAV + MYGAME.SRV)
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

## Network games: who is which character

Every machine in a network game writes **its own copy** of the save into
`Resources/gamedata/network/` (`NAME.SAV` + `NAME.SRV`). The copies hold the
same world. Two things differ:

* **Character kind** (`+0x02`): in each copy, the character of the player
  who owns that copy is `6`, the other humans are `7`, and computer
  dynasties are `5`.
* **Header**: the owner's name, gender, religion, profession, and coat of
  arms. The lobby uses these as that player's identity when they rejoin.

When loading, the game makes the kind-`6` character the local player
(`FUN_00529910`). So a reconnecting player gets their character because
they load their own copy. The lobby doesn't map players to characters
explicitly.

The header also holds:

* `+0x31`: the number of human players. The host waits for that many
  players when it loads the save (`FUN_00520e00`), and the game splits the
  AI work across that many machines.
* `+0x78`: eight checksums, one per human player's copy. A client lists a
  hosted saved game only if its own copy has the same checksum list
  (`FUN_00521130`), so all copies must come from the same save.

A single file can tell you which character its owner plays (kind `6`) and
the names of the others (kind `7`). It can't tell you which machine or
person plays each kind-`7` character.

Every player needs their own copy to rejoin. The join list shows a saved
game only if the client has `gamedata/network/NAME.SAV` with the host's
checksum list (`FUN_00521130`). When loading, every machine reads its own
local `.SAV` (`FUN_004ff4c0` → `sv_LoadWorld`). The host also streams its
`.SRV` to the others (`ls_SendGame`, `FUN_0050ecc0`), but only the new-game
path reads the received data (`FUN_0050ee10`); the saved-game path never
does. Only a new game is sent over the network: the host transfers the city
file.

The tool edits both `.SAV` and `.SRV` so they stay consistent.

## What `to-ai` changes

This mirrors what the game does with a player's character in `FUN_00527c60`:

* The kind byte goes from `7` (or `6`) to `5` (AI).
* The eight per-character action slots at `+0x5c` are reset to `-1`.
* That character's pending notices that need a human decision are removed
  from the `.SAV` (kinds `0x11 0x2d 0x2e 0x30 0x35 0x41 0x42 0x46 0x4b 0x5a
  0x5b 0x5c 0x5d 0x60`; `--keep-notices` keeps them).
* The header's human-player count (`+0x31`) is set to the new number of
  human characters. The checksum list is left alone.

`to-human` does the reverse: it sets kind `7` and raises the count.

**To continue without a player**, apply the same `to-ai` edit to every
remaining player's copy, both `.SAV` and `.SRV`, then load as usual. The
copies must stay identical apart from each owner's own kind `6`, because
the game simulates all of them in lockstep.

The character keeps its name, family, dynasty, buildings, and money.

## Caveats

* Only tested by parsing the result again. Try the edited saves in the game
  before relying on them.
* Converting a kind-`6` character leaves whoever loads that copy without
  a character of their own. The tool warns about this.

## Format notes

The full write-up of the reverse-engineering findings (file layout, header,
character kinds, dynamic state, network save/join/load behaviour, open
questions) is in [`docs/savegame-format.md`](docs/savegame-format.md). Short
version:

* `.SAV`/`.SRV`: gzip stream. Inside is the world-state format of the `.cty`
  city files (header flags `5` = save, `6` = snapshot), then the embedded
  `.ed3` city scene, then (in `.SAV` only) about 8 MB of dynamic simulation
  state.
* Characters are 0x21c-byte records in memory (array at `0x79cd40`).
  Player kinds: `5` AI dynasty, `6` human playing on this machine, `7`
  human on another machine. When saving, the network player count is the
  number of `6`/`7` characters (`FUN_005c55c0`). The game's debug name
  table at `0x637d70` (`PLAYER`, `REMOTE_PLAYER`, `RELATIVE_PLAYER`, …)
  does not match this. `15` = dead (`gm_KillSpieler`).
