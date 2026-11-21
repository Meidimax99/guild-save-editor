# Die Gilde Gold — savegames and network games

What was reverse-engineered from `GildeGold_TL.exe` (The Guild Gold, German
2.05) for this tool. Addresses are virtual addresses in that executable; names
like `sv_LoadWorld` come from the `gilde-rs` project's `re/names.tsv`, the
rest are Ghidra's `FUN_xxxxxxxx`.

Confidence markers: **verified** = checked against real save files;
**from code** = read from the decompiled loader/saver but not exercised on
data; **open** = not known.

---

## 1. Files

| File | Written by | Content |
|---|---|---|
| `Resources/gamedata/saves/NAME.SAV` | single-player save | full state (header flag `5`) |
| `Resources/gamedata/saves/NAME.SRV` | single-player save | snapshot: world + city scene, no dynamic state (flag `6`) |
| `Resources/gamedata/network/NAME.SAV/.SRV` | network save, **on every machine** | same formats; see §6 |
| `Resources/gamedata/saves/autosave.sav` | autosave (`game_Session`) | same format |

Both files are **gzip** streams (the engine's VFS mode `Z`). **verified**

The decompressed content is the world-state format also used by the shipped
`.cty` city files:

```
header
world sections (globals, objects, buildings, dynasties, characters,
                markets/cities, offices)
embedded .ed3 city scene
dynamic simulation state          (.SAV only)
```

The `.SRV` is a byte-for-byte prefix of the `.SAV`: it ends exactly where
the embedded scene ends. **verified**
Save version of Gold 2.05: `0x1005a`. The loader accepts `0x10026..0x1005a`
and has branches for older versions.

Loader: `sv_LoadWorld` (`0x50abf0`) reads everything field by field through
an `fread` wrapper (`FUN_0050abc0`). Saver: `sv_SaveWorld` (`0x507350`);
header writer `FUN_00507760`.

## 2. Header

Offsets are file offsets in the decompressed data (v `0x1005a`). **verified**
for the layout. The meaning of the owner fields is **from code**
(`FUN_00507760`).

| Offset | Size | Meaning |
|---|---|---|
| `0x00` | 4 | version (`0x1005a`) |
| `0x04` | 1 | flags: `5` = full save, `6` = snapshot; bit 1 = city form (no dynamic state) |
| `0x05` | 32 | city name (`Nuernberg`) |
| `0x25` | 8 | ? |
| `0x2d` | 1 | ? (8 in the sample saves; possibly max players) |
| `0x2e` | 1 | **number of human players** in a network game (`DAT_0064db2c`) |
| `0x2f` | 14 | game date (round, hour, minute, second) |
| `0x3d` | 16 | **owner's first name**: the character playing on the saving machine |
| `0x4d` | 16 | **owner's last name** |
| … | | owner's gender, religion, profession, coat of arms (`+0x94` of the character, stored with `+0x59a` offset), more owner bytes, a computed u32 for the owner |
| … | 8 × 4 | **checksum list** (header struct `+0x78`), one per human player's copy, unused slots 0 |
| … | `0xe100` | 160 × 120 RGB thumbnail (a screenshot of the saving machine) |
| … | 4 | ? (`DAT_0071d8d4`) |
| … | `0x60` | save name ("description", typed in the save dialog) |

`world.rs::read_header` has the exact field order. The "header struct"
offsets (`+0x30`, `+0x31`, `+0x78`) are those of the in-memory struct that
`sv_ReadHeader` (`0x50b180`) fills; the first `0x78` bytes of that struct are
what gets checksummed (§6.2).

## 3. World sections

Parsed by `src/world.rs` (copied from `gilde-rs`), in loader order. Sections:
globals, objects (2521 in the sample), buildings (84), dynasties (16 slots),
characters (~295), markets and cities, offices (42). **verified**: all
sample saves and shipped `.cty` files parse to their exact end.

### 3.1 Characters

In memory: 0x21c-byte records in an array at `0x79cd40` (`g_characters`,
0x300 slots). Fields used here:

| Memory offset | Meaning |
|---|---|
| `+0x00` | id = slot index (u16) |
| `+0x02` | **kind** (see below) |
| `+0x04` | global object id |
| `+0x08` | alive / active flag (100 at creation, 0 when dead) |
| `+0x09` | gender |
| `+0x30` | first name (16 bytes) |
| `+0x40` | last name (16 bytes) |
| `+0x50` | dynasty id (`0x8000 + n`) |
| `+0x5c` | 8 × u32 action/handle slots (reset to -1 when converting a player) |
| `+0x20c` | id of the meeting/session (council, court) the character is attending, -1 if none. Stored right after the id in the file. **Not** a network slot |

The characters section also stores `DAT_0071da90`, the **local player's
character index** (`FUN_00508750`).

### 3.2 Character kinds

| Kind | Meaning | Evidence |
|---|---|---|
| 0 | ordinary NPC | |
| 1, 2 | employees / workers | |
| 3 | politician NPC | |
| **5** | **AI dynasty head** | Computer dynasties; what the game writes when it hands a player's character to the computer (`FUN_00527c60`) |
| **6** | **human playing on this machine** | the local player is the kind-6 character (`FUN_00529910`, `game_Session`); hot-seat games rotate between several kind-6 characters |
| **7** | **human playing on another machine** | counted with 6 as network players when saving (`FUN_005c55c0`); their employees are not processed locally (`FUN_0046cd20`) |
| 8 | family member | |
| 15 | dead (`gm_KillSpieler`, `0x4f1e30`) | |

The debug name table at `0x637d70` (`DUMMY_NPC`, `MEISTER_NPC`,
`POLITICIAN_NPC`, -, -, `PLAYER`, `REMOTE_PLAYER`, `RELATIVE_PLAYER`,
`USED_PLAYER`, `INVISIBLE_NPC`) does **not** match the behaviour above and
looks like a leftover from an earlier design.

Sample network save, saved on Amadeus Abdomen's machine: id 1 kind 6, ids 2–7
kind 7, id 26 kind 5. The header records 7 human players. **verified**

## 4. Embedded scene

An `.ed3` scene in the format of `gilde-rs`'s `scene.rs`. Only its length is
needed here (`Scene::parse_prefix`). **verified**

## 5. Dynamic state (`.SAV` only)

About 8.4 MB after the scene. These are the loaders that `sv_LoadWorld` calls
in order (v = save version). Field names are in-memory offsets.

Sections 1–5 are **verified**: the editor walks them on every save and finds
plausible notices with German texts, and the checks after them pass. Sections
6–17 are **from code**. A complete tail parser written from these notes
stopped 15734 bytes short of the end of both sample saves, so at least one
layout below is incomplete; the 4-byte header difference in §9 below accounts
for only a small part of it.

1. **`FUN_004e0d00`**: u32 magic `0x1d`; 29 × u32; u32. If v > `0x10040`:
   u32 n1 (default 0x80) and u32 n2 (default 0x200). Then n1 × 41-byte
   records {u32, date[14], u32, u32, u16, u8, 3 × u32} and n2 × {u32, u32}.
2. **`FUN_0050d8e0`** (v ≥ `0x10016`): n × n byte grid, with n = 0x300
   (v > `0x10041`), 0x200 (≥ `0x1003a`), else 0x100. A second grid of the
   same size follows if v > `0x10035`.
3. **`FUN_0050d9b0`**: 12 × u32, u16, 13 × u32; 16 × 5 × u32; 5 × 0x44
   bytes; 8 × 8 × {u16, u16, u8, u32}.
4. **`FUN_004db440`** (v ≥ `0x1002a`): u32 count (32); count × {u32 id, u8}.
5. **`FUN_0050de20`: pending notices / message boxes** (0x14c bytes in
   memory at `0x726d00`):
   - u32 n, u32 payload size.
   - n × records: {u8 kind, u32 `+04`, u16 **character slot** `+08`,
     u32 `+0c`, u32 `+10`, 0x30 bytes `+14`, 3 × date[14] `+44/+52/+60`,
     u32 `+70` (timeout; -1 expires it), u8 `+78` (flags; bit 3 = open
     message box)}.
   - Each record continues with: if v > `0x1002e`, 8 × u32 `+8c`; u32 text
     length and that many text bytes (ESC markup); then *payload size* bytes.
   - Records are packed and loaded sequentially into the array, so removing
     one only means dropping its bytes and decrementing n.
   - Then: u32 n2 × {u32, u8, u32, u32, u32, date[14], 0x80 bytes}
     (`0x77a3a0`); u32 n3 × {u8, u32, u32, u32, 0x40 bytes} (`0x784e60`).
6. **`FUN_0050e290`**: u32 n; n × {u32, u8, u32, u8, u8, (v < `0x10059`:
   16 × 0x40 bytes, else 16 × u32), u8, 0x98 bytes, (v ≥ `0x1003f`)
   date[14], (v ≥ `0x10040`) u32}. 0x114-byte meeting/session records at
   `0x9516e0`.
7. **`FUN_0050e490`**: u8; u32; u32; 16 bytes; 4 × (u32 + 8 × {u32, u32}).
8. **`FUN_0050e5a0`: timed message queues**. The records are 0x99 bytes and
   share their layout with `msg_DispatchQueue`. Two links at `+0x91/+0x95`
   are stored as indices and become pointers on load.
   - 8192 records, u32, u32.
   - 0x8000 records (0x2000 if v < `0x1003c`), u32, u32. This looks like the
     network send queue.
   - The same count × 10-byte acknowledgement slots.
   - 5 × u32.
9. **`FUN_0050e880`** (v ≥ `0x1002e`): u32; 24 × 4 × u32. If v > `0x1004f`,
   two more loaders follow:
   - `FUN_005c9fb0`: 3 × u32, 5 × 0x60 bytes, u32.
   - `FUN_004dbae0`, header: u32, u16, u32, u32, u32, u32, 12 bytes, u32,
     u32, u32 (46 bytes).
   - `FUN_004dbae0`, then 8 records of 284 bytes each (array at `0x71c318`
     with stride 0x170): u32, u32, (v ≥ `0x10055`: 2 floats), u32,
     4 × 16 bytes, 4 × u32, u32, 4 × 16, 4 × u32, u32, 4 × 16, 4 × u32,
     u32, 12 bytes.
10. **`FUN_0050e940`**: reads nothing from the file.
11. **`FUN_0050cb90`: 3D actors** (characters and carts in the scene):
    - u32 n1 × actor and u32 n2 × actor, read by `FUN_0050c9e0`. An actor is
      {u32 id, name[0x20], u32 building, u32, u16 flags, model[0x30],
      mesh[0x40], position 4 × f32, rotation 3 × f32, u8 (1 = flag 0x20),
      u32 owner, (v > `0x10012`) 0x40 bytes, (v > `0x10059`) 0x40 bytes}.
    - Then N activity records (N = 2000 for v > `0x10057`, 1280 for
      v ≥ `0x10044`, else 0x300). Each is {u8 kind, u32, u32 actor,
      0x20 bytes, 0x20 bytes, 0xa0 bytes, u8, u32 next, u32 prev}, with
      0x194 bytes in memory each.
12. **`FUN_004a4810`**: u8; 128 × {u32, u32, date[14], u32, u32, u8}.
13. **`FUN_00578020`** (v ≥ `0x10029`): u32 n × {u8, u32, u32}.
14. **`FUN_005969f0`: production settings** (v > `0x10046`): u32 n ×
    {u32, u32, u32, 7 × {u32, u8}}.
15. **`FUN_004da310`** (v > `0x10048`): 5 × u32, u16; 3 × {u32, u32, u32,
    5 × u8, u32}.
16. **`FUN_0049bee0`** (v ≥ `0x10052`): 5 × u32.
17. **`FUN_004ae3c0`: routes** (v > `0x10052`): u32 n × {u32, u32,
    0x18 bytes, 0x18 bytes}.

## 6. Network games

### 6.1 Saving: every machine writes its own copy

`FUN_005c55c0` runs on every machine:

1. It counts the network players as the number of characters with kind 6 or
   7 (`DAT_0064db2c`).
2. It reads its own header, computes a checksum over the first `0x78` bytes
   of the header struct (`FUN_00401000`), and broadcasts {own character id,
   checksum} (network command `0x10`).
3. It waits until it has collected one entry from every player
   (table `0x79bbe0`: 8 × {id, checksum}).
4. Each machine then writes `gamedata/network/NAME.SRV` (flag 6) and
   `NAME.SAV` (flag 5) itself.

As a result the copies **share the world** but differ in:

| Differs per copy | |
|---|---|
| character kinds | the owner is 6, the other humans are 7 |
| local player index | characters section |
| header owner fields | name, gender, religion, profession, coat of arms, … |
| thumbnail | that machine's screen |
| network bookkeeping | probably the send queue / acknowledgements in dynamic section 8 (**open**) |

| Identical in all copies | |
|---|---|
| header human count (`+0x31`) | |
| header checksum list (`+0x78`) | holds all players' checksums |
| save name, date, world | world assumed identical because the game runs in lockstep (**open** until a client copy is compared) |

### 6.2 Rejoining: the lobby needs the local copy

`FUN_00521130` (join browser, `Menu\SEARCH_NETWORK`):
- For each saved game a host advertises (type `0x7f`), the client opens its
  own `Gamedata\network\NAME.SAV`.
- The game is listed only if that file exists and its header checksum list
  equals the host's (compared for header `+0x31` entries).
- When joining, the client's identity (first/last name, coat of arms,
  gender, religion, profession) is taken from **its own copy's header**
  instead of the `[Network]` section of `game.ini` (`FUN_005783a0`,
  lobby `menu\netlobby`).

`FUN_00520e00` (host, load network game): reads the host's own header and
takes header `+0x31` as the number of players to wait for (`DAT_0064db2c`).

### 6.3 Loading

`FUN_004ff4c0`:
- The host streams its `.SRV` to the others in 0x80-byte chunks
  (`ls_SendGame`, `FUN_0050ecc0`), followed by command 6 with the player
  count.
- Then **every machine loads its own local `.SAV`** (`sv_LoadWorld(...,5)`).
- The streamed data is only consumed by `FUN_0050ee10`, which is used in the
  **new-game** path (`FUN_004ff040`, where the host sends the city file). The
  saved-game path never calls it.

So no save is ever transferred: **each player needs their own copy**.

### 6.4 Who plays which character

There is no explicit lobby-slot-to-character mapping. Each machine loads its
own copy and makes the kind-6 character the local player (`FUN_00529910`).
A player gets their character back simply because their copy marks it as 6.

From a single file one can tell:
- whose copy it is: the kind-6 character, and the header owner name, which
  matches it;
- the other humans by name (kind 7).

One cannot tell which machine or person plays each kind-7 character, other
than by finding the copy in which that character is kind 6.

**Open:** how the host binds a joining connection to a character. Its
identity comes from that player's header, but no name comparison against the
characters was found.

### 6.5 Simulation split

`FUN_0046cc60`, `FUN_0046ccb0`, `FUN_0046cd20`:
- A character is processed locally if the game is not networked, or if it
  is kind 6.
- Otherwise it is processed locally if `object id % player count ==
  this client's index` (`DAT_01470ba0` / `DAT_0064db2c` / `DAT_01470bb4`).
- Employees of kind-7 players are never processed by the AI routines.

This is why the header count `+0x31` must match the number of human players
that actually connect: otherwise the work assigned to a missing client index
is never done.

### 6.6 What the game does when a player leaves

`game_Session` ends a network game when a player disconnects ("Ein Spieler
hat die Partie verlassen. Das Spiel wurde beendet…"). `FUN_00527c60`
converts a human's character into an AI one:
- kind → 5;
- a new random first name and a new dynasty id;
- the eight `+0x5c` slots set to -1;
- that character's notices of kinds `0x11 0x2d 0x2e 0x30 0x35 0x41 0x42
  0x46 0x4b 0x5a 0x5b 0x5c 0x5d 0x60` cancelled (`FUN_0052c810`);
- related meeting records (`0x9516e0`) closed.

If no living kind-6 character is left, it sets game over (`DAT_0071da74`).

## 7. What the editor changes

`to-ai` on one copy:
1. Sets the character's kind from 6/7 to 5.
2. Sets its eight `+0x5c` slots to -1.
3. Removes its pending notices of the kinds listed in §6.6 from dynamic
   section 5 (`.SAV` only; `--keep-notices` keeps them).
4. Sets header `+0x2e` (the human count) to the number of remaining kind 6/7
   characters.

Deliberately unchanged:
- **The checksum list.** The lobby compares the stored lists, and all copies
  still carry the same one.
- **Name, dynasty and family.** Unlike the game, the character keeps them.

The same edit must be applied to **every remaining player's copy**, both
`.SAV` and `.SRV`.

The session tool groups copies by (version, city, date, checksum list) and
identifies each copy's owner by its kind-6 character.

**Verified:** edited files decompress, parse, and differ from the originals
only in the bytes listed above. **Not yet verified in the game.**

## 8. Open questions

- Byte-level comparison of a host copy and a client copy of the same save.
- The exact layout of dynamic sections 6–17 (see the 15734-byte gap).
- Header bytes `0x25..0x2d` and the unnamed owner fields.
- Whether the game rejects a copy whose own checksum no longer matches its
  entry in the list. Editing `+0x2e` changes the checksummed bytes; no check
  of a copy's own checksum was found.
- How the host maps joining connections to characters (§6.4).
- Whether the type-`0x7f` lobby check uses header `+0x31` or the advertised
  count when the two differ.
