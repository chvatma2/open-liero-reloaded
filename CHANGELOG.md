# Changelog

All notable changes to Open Liero Reloaded (OLR) are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/).

## Unreleased

Work towards 0.2.0: new formats, level, mod and skin editors, a modern look,
and Liero Factory Reloaded and Liero v1.0 Reloaded
(`docs/superpowers/specs/2026-10-16-milestone-0.2-scope.md`).

### Added

- Pack authoring tools: `olr-cli pack check` (validate packs), `pack fmt`
  (rewrite them in canonical form; `--check` lists what is not) and `pack
  new` (start a mod or a level, or copy any pack).
- Level operations: `olr-cli level carve|fill|paint|stamp|resize` and
  `level object list|add|move|remove`, in world pixels, each saving the
  whole pack at once; `olr-cli level preview` draws a level to a PNG.
- Format documentation in `docs/formats/`: a guide with a checked example
  of each pack kind, and reference pages generated from the code.
- Levels can be composed of elements (imported images, drawn layers) and
  baked: `olr-cli level element …` and `level bake`. PNG and JPEG pictures
  are pixelated, colour-reduced and dithered; `pack check` reports a bake
  that no longer matches its composition.
- The Linux release is a Flatpak:
  `flatpak install --user open-liero-reloaded-<version>-linux-x64.flatpak`
  puts the game in the desktop's menu. It replaces the Linux zip; Windows
  keeps its zip.
- The level editor's core (`olr-editor`): elements, painting, spawns,
  undo and redo, tested headless. Imported JPEG photos are turned by their
  EXIF orientation.
- The level editor in the game: Editor in the main menu (and `olr --edit
  <dir>`), painting, images, spawns, Play and recovery. Levels you make
  live in the data directory's `packs/`; a bundled level is edited as a
  copy. Play tries the level at once (alone, or against up to 8 bots);
  Esc or Stop returns to the editor as it was. Unsaved changes are kept
  in a recovery copy every 2 minutes and offered after a crash.
  Import image… previews the picture you select.
  A loaded map opens as three layers, rock, dirt and background, to erase
  and paint (a level composed before: Split into layers).

- Releases say what they are: a work in progress is named
  `<version>-dev.<commits>.<commit>` (such as `0.2.0-dev.192.24b2ac4`), a
  final release (`cargo xtask dist --final`, from a commit tagged
  `v<version>`) the plain version. The game and the server show it
  (`--version`, About, the main menu, their logs).
- Lit levels: a level can set darkness (an ambient colour and strength)
  and make any pixels glow (steady, pulsing, flickering or keyed): glowing
  pixels are a level's light. Light spreads from their shapes with soft
  shadows, bounces off rock and dirt in their colour (daylight through an
  opening lights a room), never passes rock or dirt however thin, soaks
  into solids, which never fall below a solid floor nor show brighter
  than their art, and bright light glows in the open (bloom, with a
  smooth tone curve up to the level's max light).
  Each separately drawn flame of a flickering group flickers on its own.
  Clear solids let light through. Explosions, muzzle flashes and glowing
  shots light the level as small glowing discs. Worms, shots and bonuses
  stay visible in the dark. A level without a look draws as before.
- Worms in the light: in a lit match a worm is lit on the side a light
  comes from, throws a soft shadow, and never shows below a floor of 35 %
  of the level's ambient hue.
- The level editor makes lit levels: a Look panel, the Glow brush (H),
  lit, full-bright and light-only views and a preview clock. Pixels picked
  by colour or by a rectangle can be made to glow. Spawns copy and paste.
- `olr-cli level look`, `level group`, `level element glow` (a shape or
  `--colour-at`) and `look-preview` make and check lit levels from the
  command line.
- Idle bots: a bot personality that does nothing (Local Play's
  Personality, `--personality idle` on the command line), to explore a
  level with worms around.
- A playtest pack, Castle Strike by day (`content/playtest/`): daylight
  through the hall, the loft and the towers, flickering torches and
  candles in the caverns.
- Your own worm no longer shows its name above it (OpenLieroX shows none);
  its weapon's name still shows while you change weapons.
- A log file of each run in the data directory's `logs/` (the last 10
  kept, 2 MiB each), for bug reports: the version, system, GPU and video
  settings, matches, online events and errors, and a crash's message,
  place and backtrace. Home paths show as `~`; addresses and keys are left
  out. `--log debug` or `OLR_LOG=debug` logs more. The dedicated server
  writes a crash into `server.log` too.
- **Report a bug** in the main menu (release builds with a bug relay):
  say what happened and what you expected, attach a saved replay if you
  like, read the preview of everything that will be sent (the version,
  system, GPU and display, the settings in effect, this run's and the
  previous run's logs, the replay's worm names), then Send. It becomes a
  public GitHub issue, filed by the relay's bot (`tools/bug-relay/`); no
  account is needed and nothing is sent until you press Send.
- **Updates from the main menu** (release builds): **Update available:
  <version>** opens the release's notes and **Update**. The Windows zip
  downloads, verifies its signature and replaces itself in place, keeping
  your files, then restarts on request; the Flatpak updates through the
  Flatpak portal from the game's own repository
  (`https://openlieroreloaded.github.io/flatpak/`, branches `stable` and
  `beta`). Options → Online → **Include pre-releases**. `cargo xtask
  publish` signs and publishes a release (`docs/releasing.md`).
- Loadouts: which weapons are on, their loading time and a Unique rule
  (no weapon twice in a worm's five slots unless every weapon on is
  already used), named per mod. Every mod has Default (every weapon,
  100 %) and read-only loadouts made from OLX's presets; Edit loadouts…
  in Local Play and the lobby makes your own, kept in `loadouts.ron`. An
  online game plays the host's loadout; a server's rotation entry may
  name one (`loadout: "Rifle only"`).
- Two Reloaded levels ship with the game: **Liero Factory Reloaded** (OLX's
  Liero Factory at night, its lamps, screens and displays glowing; it
  plays as before) and **Castle Strike Reloaded** (Castle Strike by day,
  lit by the sky). They are OLR's own packs in `content/reloaded/`, beside
  the imported content, listed by the game and nameable in a server's
  rotation (the example config plays both).
- Skin tools: `olr-cli skin new` (a blank skin, or a copy of any
  installed skin), `skin double` (a classic skin at double resolution by
  Scale2x, to refine), `skin check` and `skin preview` (every frame, tinted,
  both ways, light and dark, glow shown). **Firefly**, a double-resolution
  skin made with them, ships with the game: its tail lantern glows and
  lights the worm's surroundings in lit levels. Pick it in Profiles.
- OpenLieroX's **Weapon 1 to 5** keys: holding one selects that weapon
  slot (by default 1 to 5, keyboard 2's 6 to 0; no gamepad button unless
  you bind one in Options → Keys). A key beyond the worm's slots does
  nothing. The weapon's name shows over your worm after a quick-select
  and, as in OpenLieroX, after Previous/Next weapon. Older settings files
  get the new keys; digits typed in the in-game chat select nothing.
  Online play, replays and desync bundles from earlier builds are not
  compatible.
- **Layer folders** in a level's composition, nested to any depth: an
  object's pieces are moved, hidden and locked together.
  `olr-cli level element group`, `ungroup` and `set --folder` make and
  change them, `list` shows the tree, and placing an asset of several
  pieces puts them in a folder named after it. The level editor opens and
  saves them, undoes every folder change, copies and pastes a folder with
  its subfolders, and places an asset of several pieces in a folder.
  Its Elements list shows them as a tree: fold a folder, hide or lock it
  with its eye and lock, drag rows into, out of and between folders, or
  put the selection in a new folder (Ctrl+G; Ctrl+Shift+G ungroups). On
  the canvas a click selects a whole object, a double click goes inside
  it, Alt+click picks one piece and Escape goes back up.
- **Glowing distant layers**: a parallax layer may glow. Its light falls
  on the level through the openings in the back as if the camera were at
  the level's centre (once a frame, whatever the number of views), and
  the layer itself shows its art in the dark wherever it scrolls. The
  level editor's Glow controls and brush and `olr-cli level element set
  --glow`/`glow` work on a distant layer; `list` shows each element's
  glow.

### Changed

- The HUD no longer says "Jump to respawn" while you are dead: the worm
  respawns by itself.
- The game's window shows only once its first frame is drawn (no white
  rectangle while it starts).
- Skins are no longer downloaded. A player whose skin you don't have, or
  whose skin differs from yours, appears with the default skin. The
  network protocol version is 3: 0.1 clients cannot join 0.2 games.
- Replays record each worm's name, colour and skin, so a replay shows the
  players apart. Replay format 4: older replays no longer open.
- Worms facing left are drawn as the exact mirror of facing right, without
  OLX's one-pixel offset.
- In lit levels worms no longer cast shadows or have a lit side: a worm
  takes the light where it stands, never darker than its floor.
- In lit levels a soft dark halo on the background behind each worm keeps
  it readable: tight on the level's back wall, wider and fainter on far
  distant layers, none on the farthest (a night sky), on rock or on
  glowing pixels.
- Explosions heat only what they reach: the open back wall and the
  outer two pixels of the rock or dirt they meet, from which the heat
  soaks in. No more glow deep inside rock, in a tunnel behind a wall or
  on a wall's far face.
- A surface material may set its own light soak (`light_soak`, 0 to 12
  px; the material editor's Light soak): how deep light seeps into its
  rock and dirt. Concrete, stone and brick soak 2 px, so Liero Factory
  Reloaded's slabs and Castle Strike Reloaded's stone walls are lit only
  at their face, no longer in blobs at the lamps' corners; furniture and
  everything else keeps the level's soak.

- Content packs are in the new format, version 5. Re-import packs you
  made with `cargo xtask fetch-olx-content` or `olr-cli import`; packs of
  another version are refused. Content hashes changed, so 0.1 replays and
  cached downloads no longer open.
- Local Play is simpler: mod, level, loadout, lives, kill limit, time
  limit, your profile, the bots and their difficulty; each bot's own
  difficulty, their personality and weapons are under Advanced…, also in
  the lobby. A fresh install and a missing loadout play Default; an
  older settings file's weapon preset and loading time are not carried
  over. The network protocol version is 4.

### Removed

- `olr-cli validate`: use `olr-cli pack check`.
- Bonuses in play: the Bonuses checkbox, `olr --bonuses`, the server's
  `bonuses` rotation field (ignored, with a warning) and `/settings
  bonuses`. `olr-cli` keeps them for tests. Also gone: Local Play's
  loading time slider, weapon preset and "Reset the match to OLX's
  settings".
- The main menu's Sandbox: play Local Play against bots of the Idle
  personality instead (`olr --mode sandbox` still starts it).
- The Import screen; import OLX content with `olr-cli import --all` (it adds packs; `--prune` removes the others), and later in
  the editors.

### Fixed

- Every menu screen, Options tab and dialog now fits the window at UI
  scale 3 in fullscreen on 16:10 and 4:3 screens: the Video tab, Local
  Play, the lobby, the profiles, Keys and the editors put side-by-side
  panels one under the other, and a window too short for a screen
  scrolls it. Keyboard and gamepad focus scroll every control into view.
- In split screen the pause menu and the results sit at the window's
  centre, not over the first player's view.
- The skin editor fits every window: its frames shrink to the panel, its
  text wraps, and a narrow window shows the tools or the frames beside the
  canvas (Tools | Frames); a control focused with Tab scrolls into view.
- Each mod now plays with its own worm and rope physics, as in OpenLieroX.
  Modern Warfare 1.0 had been playing with Liero v1.0's gravity, jump and
  ninja rope; its worms now fall more slowly, jump higher and its rope pulls
  as MW intends.
- A worm standing on the ground no longer bobs up and down a pixel, and
  neither does the camera following it: with smooth movement on, a worm on
  the ground is drawn at whole pixels vertically, as in OpenLieroX.

## 0.1.0 - 2026-10-16

The baseline for later work: Open Liero Reloaded's first release under its
own name. It plays a Deathmatch against OLX's bots locally and online: games
are found on Nostr relays, LAN and invite codes, connect through routers
(UPnP, hole punching, TURN relays), survive their host leaving (host
migration), download missing content and keep players who leave from
freezing the match. Physics, movement and shooting still reproduce OLX's
own recorded traces bit for bit. Graphics work starts in 0.2.

### Changed — the name

- The project is now **Open Liero Reloaded** (OLR; its working name was
  rust-liero). The game is `olr` (`olr.exe`), the server `olr-server`, the
  tools `olr-cli`, and the release zips
  `open-liero-reloaded-<version>-{linux,windows}-x64.zip`.
- Settings, profiles, replays and downloads live in an
  `open-liero-reloaded` data folder (`~/.local/share/open-liero-reloaded` on
  Linux). Move the old `rust-liero` folder's contents there to keep them.
- Replays are `.olrp` files and invite codes start `olr1`. Older replays,
  pack archives and invite codes, and older builds online, do not work
  with this version.

### Fixed — players leaving a match

- A player who leaves, times out or is kicked is removed from the match at
  once on every screen, as in OpenLieroX: no frozen worm is left to shoot,
  and its rope and the credit for its shots go with it; shots already
  fired still explode.
- The log says "<name> has left" or "<name> has timed out".
- The live scoreboard drops the player; the final results keep a "left"
  row with its kills and deaths so far (OpenLieroX loses the row; see the
  deviations in `docs/porting/STATUS.md`). Replays show it.
- If fewer than 2 worms are left, the match ends.
- A player whose game crashed or lost its network keeps its last move for
  up to 10 seconds, then goes. A player who leaves while the host is being
  replaced is removed when the match resumes.
- A spectator, a replay or a bots-only game that follows a worm which left
  moves on to the next one.
- Only your own rope throws are heard (OpenLieroX plays the throw sound
  for the local player only): eight bots no longer fill the mix with them.
- The results and the Tab scoreboard count every death and suicide: with 10
  lives a worm that is out shows 11 deaths, never fewer than it died
  (OpenLieroX counted none while a worm's score was 0 or below).

### Changed — leaving players

- Replays (format 3) and desync bundles from earlier builds no longer
  open, and earlier builds cannot join a game of this one (proof-of-concept
  stage). Online play uses about 100 bytes a second more.

### Added — host migration

- A match goes on when its host crashes, leaves or freezes: the players
  move it to a new host (the first player who joined, then the next).
  See "When the host leaves" in `docs/online.md`.
- **What players see:**
  - While the match moves, the top line reads "The host is gone: finding
    who takes over…", then "…moving the game to <name>…" (or "…you are
    taking over the game…"). The log then says "<name> is now hosting"
    (or "You are now hosting"); if the old host comes back first, "The
    host is back: the match goes on".
  - The new host's lobby is closed: only this match's players are taken
    back ("Taken over from the host: closed to new players"). A player
    who comes back late watches until the next match.
  - **Host lost:** when no player can take over, a screen says why and
    where the match's replay is saved (Replays, Back).
  - **Match lost:** the host whose own game stalled for 1.5 s, or whose
    network cut it off from its players ("Connection lost: the others
    play on under a new host."), sees this screen: the match moved on
    without it, and its replay is kept.
  - A migration takes about 2.5 s on a LAN; through the public relays it
    has not been measured. A match is moved at most 4 times.
  - **Two players only:** with one player and the host a short blip is a
    stall, not a migration; a host that crashes leaves the player on the
    Host lost screen after about 12 s (a host that quits hands over at
    once; with no bots the match then ends, and the player left wins).
  - **Who goes on:** the new host needs a majority of the match (players
    and spectators who were in it, and the old host unless it said
    goodbye). An even split goes on on neither side; with a host and
    three players, a host crash plus one more player out of reach is
    "Host lost" for the other two. A new host that was not reached is
    asked again every second within its turn.
  - **Known limit:** a host that quits just after a player crashed leaves
    the last player at "Host lost" about 12 seconds later: it still waits
    for the crashed player, so the match never splits in two.
  - **When it fails:** the Host lost screen says which case it was (too
    many moves, the host left while you were still joining, nobody could
    take over, with the last try), and the console and log say each
    attempt (`rl-game: migration: …`).
- **A network pump:** the game polls its online connection on its own
  thread whenever its window has not for 100 ms, so dragging or resizing
  the window, or a long match load, no longer silences a host (its
  players used to migrate away from a healthy host) or drops a client.
  Only a stalled machine does.
- `cargo xtask net-soak [--minutes N] [--seed S]` plays seeded online
  cases for N minutes (default 10), host crashes, departures and freezes
  among them, and keeps each failure as a replayable case in
  `target/failures`. It is not part of `full`.
- `rl-cli net-sim` takes `--exit peer:tick:crash|leave`,
  `--host-freeze from:until`, `--profile-change tick:peer:profile` and
  `--migration`; `net-client` migrates by default (`--no-standby` makes
  it a follower only); `cargo xtask net-nat` gains the `migration`
  scenario (a host killed behind a router).

### Fixed

- `rl-game --net-sim bad` loses datagrams one at a time, as the spec
  defines the profile, not in bursts.
- A spectator who joined mid-match whose host goes sees Host lost instead
  of hanging.
- Hovering a control no longer resizes it; the Internet games fit narrow
  windows; Start and Force start stack, and Kick and Ban wrap, when the
  window is narrow.
- Copy and paste work in the text fields, bots are counted apart from
  players in game lists ("1/8 +3 bots"), and the skin preview's speed no
  longer depends on the display's refresh rate.

### Added — through routers

- Two players behind ordinary home routers play each other with no manual
  port forwarding. See "Through routers" in `docs/online.md`.
- **Port mapping:** a hosted Internet or Invite only game, and an
  `rl-server` (Public or Invite), asks the router to open its port (UPnP,
  NAT-PMP or PCP, TCP and UDP on one port), renews it and deletes it at
  exit. A double NAT or carrier-grade NAT is told apart (the port opens,
  but the router's own address is not public). Options → Online: "Open
  the game's port on my router (UPnP/NAT-PMP/PCP)", on by default.
- **Joins without an address:** a listed game without a public address
  is joinable (its ping says "via relays"), and so is an invite code
  without one: the joiner asks the host through the Nostr relays (an
  encrypted, short-lived handshake signed with a fresh key per join), and
  ICE finds a path: the host's open port, hole punching through both
  routers, or a TURN relay. A game with an address is tried directly
  first, then through the relays. The waiting text names each step.
- **TURN relays:** `rl-server --relay` runs a volunteer TURN relay and
  announces it on the server's Nostr relays; joiners use announced relays
  when nothing else connects, the nearest region and the least loaded
  first. Options → Online → "TURN relays" takes your own
  (`user:pass@host:port`; the first two are used, before announced ones).
- **`rl-server --nat-check`** prints what stands between the machine and
  the players: its addresses, each STUN server's answer and the kind of
  NAT, a test mapping on the router, the Nostr relays and the TURN relays
  announced on them.
- **The lobby says how players get in:** the host's lobby lists its
  reachability under the invite code ("Port 23400 is open on your router
  (UPnP): players join directly at …", "Your router did not open port
  23400 (…): players connect by hole punching or a TURN relay", a
  symmetric NAT,
  "Taking joins through 5 of 8 relays"), each member shows its path
  ("direct", "open port", "punched", "relayed"), and a joiner sees
  "Connection: connected by hole punching through both routers" and the
  like. `rl-server` logs the same as `reach: …` lines and each join's
  path (`join: … via punched …`). A failed join names the step that
  failed and the fix ("no network path to the game: hole punching failed
  and no TURN relay could be used. The host can open port …").
- `rl-cli net-client` joins by `--invite`, `--key` or `--listed "<name>"`
  as well as `--join`, with `--relay`, `--stun`, `--turn`,
  `--allow-local` and `--lobby-only`, and prints `path: <tag>`.
- Developers: `cargo xtask net-nat` runs joins across Docker routers
  (port-restricted, symmetric, UPnP, PCP, NAT-PMP) in `full`; `rl-cli
  nostr-relay` and `rl-cli stun-server` (feature `nat-tools`) serve
  tests.
- **Server operators:**
  - New config keys: `port_mapping` (default `true`: a Public or Invite
    server asks the router to open its port, and with `--relay` the
    relay's ports), `allow_local` (default `false`: accept private and
    local addresses where public ones are expected, for an office or LAN
    setup) and `relay` (a TURN relay's `port`, `ports`, `capacity`,
    `user_kib_s`, `allocations_per_ip`; `--relay` uses the defaults). See
    `fixtures/server/example.ron`. Set `port_mapping: false` if the server
    must not touch the router.
  - A Public or Invite server now listens on its Nostr relays for joins
    (a second connection to each relay); a Lan server does not. An empty
    `relays` keeps it off Nostr.
  - The address in a note or invite is now the router mapping's,
    `public_address`, or, on a machine with no NAT (a VPS or cloud VM
    with a public address on its interface: STUN sees its own address),
    that address on the game's port ("reach: No router: players reach you
    directly at …"). It is never the address STUN learned through a NAT:
    a server behind a manual port forward should set `public_address`, or
    players join through the relays. A VPS keeps its address only with
    `stun` set (the default list).
  - A relay limits each player's IP to 32 KiB/s each way, 4 allocations
    and 20 requests a second (`user_kib_s`, `allocations_per_ip`), relays
    only to public addresses (not to the operator's LAN, unless
    `allow_local`), and ends every allocation after 6 hours, so a player
    relayed longer than that loses the link and joins again. It needs a
    public IPv4 address: `public_address`, a router that maps UDP 3478
    and 49160-49191 exactly, or no NAT; otherwise it logs "relay: not
    announced" and does not start. Its credentials are public by design.

### Fixed — through routers (final review)

- A flood of valid offers whose joiners send checks from forged
  addresses can no longer lock honest joiners out: a pending join counts
  as live only once it answered one of the host's own checks, at most 2
  per IP.
- A host whose ISP renumbers its router follows the new address: each
  port-mapping renewal reads the router's address again, and the invite,
  the listing and a relay on that address move with it.
- The host's lobby and log say what became of each join through the
  relays: answered, refused, answered but not connected, or ignored
  because one of the two PCs' clocks is off by more than a minute. A
  joiner's "did not answer" names the clocks, and a relay refusing the
  join's events as invalid asks after the clock. `rl-server --nat-check`
  prints each relay's clock against this PC's and whether it forwards a
  joiner's handshake.
- A relayed joiner's "connected through the TURN relay …" names the
  relay, not the host.
- A TURN relay carries games hosted behind its own public address (its
  own server's game, a player on its LAN) when the router hairpins; it
  refuses only its own ports.
- A server with a public address of its own and no NAT (a VPS) is
  listed and invited at that address again, and its log says "No router:
  players reach you directly at …".
- A stalled link (a lossy path, a loaded machine) no longer floods its
  peer with old inputs when it recovers, which dropped honest players as
  flooders; it drops its stale inputs instead.
- Smaller fixes: the port-mapping race on a router with one table for
  all protocols, PCP and NAT-PMP time after re-hosting, loopback
  candidates refused from strangers, a member's path tag following its
  link (and a relayed member never IP-banned), the host's console
  printing each member's path and its relays' state, a listener restart
  that failed, duplicate relays by a trailing `/`, and the new TURN relay
  field masked while typed.

### Added — finding games

- Multiplayer has three tabs: **Internet** (games announced on public Nostr
  relays: name, map, mod, players, ping, region and a lock for a password;
  games of another version greyed out, games without a public address "cannot
  be joined yet", which "Added — through routers" above lifts: they join
  through the relays; Join, and Block this host), **LAN** and **Direct** (an
  address or an invite code). See `docs/online.md`.
- Invite codes (`rlx1…`): a hosted game's lobby shows its code with a Copy
  button; pasting one in Direct joins that game and only that game.
- Visibility, chosen when hosting ("Who can join"): Anyone (Internet), This
  network or Invite only. `rl-server` takes `visibility: Public`, `Lan` or
  `Invite` (`DirectOnly` still reads as `Invite`) and now defaults to Public.
- Options → Online: region, public address, the relays and the STUN servers
  (with Reset to defaults), and the blocked hosts.
- `rl-server` announces itself on the relays (`relays`, `region`), learns its
  public address from the STUN servers (`stun`) or `public_address`, prints
  its invite code in the log and answers `/invite`.
- A joiner with an invite or a listed game checks the host's key: a host that
  cannot prove it is refused ("this is not the game you were invited to"). A
  host that never answers is given up on after 10 s.
- `rl-cli nostr-probe` and `rl-cli stun-probe` measure public relays and STUN
  servers (by hand: the relay probe publishes short-lived notes to them).
- Still needed: players outside your LAN reach a game only through a
  forwarded port (NAT traversal comes with plan 12b: lifted by "Added —
  through routers" above, which also makes games without a public address
  joinable).
- Privacy: opening Multiplayer connects to the relays in Options (8 public
  relays by default), which see your IP address; remove every relay to
  stay off them. Upgrading `rl-server`: a config without `visibility` is
  now Public and announces the server on the default public relays (set
  `visibility: Lan` or `Invite` to keep it private).

### Fixed — finding games (final review)

- A closed game no longer comes back for up to 3 minutes from a relay that
  missed its closing note, and a game hosted again right after closing
  shows at once.
- Announcements are dated by the wall clock at each note: a host that
  suspended or whose clock was corrected by NTP after it started is no
  longer dropped from every list; the game list judges ages the same way.
- A game's password proof is bound to the joiner and the connection and
  sent only once the host proved its key: an impostor can no longer relay
  it into a password-protected game, and an unproven "wrong password" is
  not believed.
- Relay edits in Options apply at once, also to the relays kept open after
  leaving Multiplayer and to a hosted Public game (which stops announcing
  on removed relays); blocking and unblocking apply at once.
- An Invite only game says when its code has no address ("This code has no
  address: forward port N or set the public address in Options").
- Left/Right follow another worm in `rl-game --spectate` and `--replay`.
- "Listed on the Internet" no longer lingers after every relay dropped or
  from a previous hosted game; a private or IPv6 public address is never
  announced on public relays (the game says so; `rl-server` warns).
- A failed host proof on a LAN or direct join says "this game could not
  prove who it is" (nobody was invited).
- An invite code with a typo in `rlx1` says "damaged" instead of being
  looked up as a host name; the Internet tab forgets a password typed for
  another game.
- `rl-server` checks `stun` entries (`host:port`), holds `relays` to 32,
  each once; the example config leaves both lists out so a copy follows
  later defaults.
- Relay connections: events a relay sends outside our subscription are
  dropped and the inbox is bounded by memory; reconnects are spread out
  (±20 %); STUN survives a stale ICMP reset on Windows.

### Added — content download

- A player who lacks a game's mod, level, theme or a player's skin
  downloads it from the host in the lobby: a "Downloads" list with
  progress, Ready once the content is complete. More than 50 MB still to
  download asks first ("This game needs N MB of content from the host").
  Downloads are verified, resume where they stopped and are kept in a
  download cache (1 GB at most, the least recently used dropped first);
  Options → Online shows its size and clears it. See `docs/online.md`.
- Skins travel through the host: every player sees everyone's skin, even
  one only its wearer has.
- A player still downloading when a match starts waits it out in the
  lobby, connected, and joins the next one (or watches once complete)
  instead of being dropped.
- `rl-server` serves its packs to players who lack them, keeps their
  skins in `<data dir>/downloads`, and sends at most `upload_kib_s`
  (default 4096 KiB/s) in all.
- `rl-cli net-client --cache <dir>`: bot clients download what their
  `--content` lacks (a temporary cache by default).
- Replays of matches on downloaded content play back, from the Replays
  screen and from `--replay`.

### Added — release zips

- `cargo xtask dist` builds portable zips for Linux and Windows,
  `target/release-zips/rust-liero-<version>-<platform>.zip`: `rl-game`,
  `rl-server`, the content, the docs and an example server config in one
  folder, to unpack and run anywhere. It unpacks and runs each zip to
  check it (`--no-verify` skips that).
- `rl-game` and `rl-server` without `--content` use the `content/packs`
  beside the program when there is one, else the one in the working
  directory as before.

### Added — choosing the worm to watch

- Left/Right choose the worm the camera and the HUD follow in any game
  nobody plays: a replay, a `--spectate` match and an online match you
  watch (before, only the last, with PageUp/PageDown). PageUp/PageDown
  change the speed only; online they do nothing. A replay starts on the
  worm of the player who recorded it (worm 0 for a match of bots alone).

### Added — in game

- While the change-weapon key is held, the selected weapon's name shows
  above your worm, in place of your name, and stays for 0.75 s after a
  change, as in OLX.

### Changed — more effects on screen

- Up to 16 384 effects (blood, gibs, dirt, smoke, explosions) at once
  instead of OpenLieroX's 1024, so busy fights no longer lose their
  blood and gibs. The last 1024 are kept for deaths, so a death's blood
  explosion always shows. The room is set aside once when a match
  starts. A 16-bot chaos match peaks at about 7 600 effects; with 32
  bots firing everything a frame takes up to about 1-2 ms more on the
  CPU, well within 144 Hz.

### Fixed — hits and deaths

- Shots hit a worm where it is. About one hit in ten tested where the worm
  had been a second earlier (an OpenLieroX bug the port had kept on
  purpose): worms died to shots that visibly missed, and shots passed
  through worms. The oracle still reproduces OLX's behaviour.
- A worm left at exactly 0 health now dies with the death sound and the
  blood explosion, like any other death, and the kill goes to whoever
  dealt the last damage (OLX credited nobody and showed nothing). This
  holds in the seconds after the game over too.
- Every death shows in full: its blood is never dropped when a busy
  moment fills the effects, and online a correction that moves a death
  to another moment no longer shows it twice.
- Every death is heard: when two of the same death sound already play,
  the oldest makes way (a 5 ms fade) instead of the new one being
  skipped.
- Replays recorded before this change no longer play (replay format 2).

### Changed

- Online play's protocol version is 2: this version and the earlier one
  cannot play together.
- Disk use of builds: dependencies are built without debug info in dev,
  tests are one binary per crate, and `cargo xtask check` sweeps
  superseded builds automatically.
- Release builds use whole-program optimisation (fat LTO, one codegen
  unit). Small hot paths got up to 12 % faster (32 worms' tick, the draw
  list); the heavy ticks and frames are unchanged within noise. A release
  rebuild takes about two minutes.
- The menus are centred: every screen the main menu opens is one column in
  the middle of the window, its title on top, its sections in panels and
  its main button (Start, Import) large and centred at the bottom. Local
  Play shows the map on the left and the match and players on the right,
  with "Reset the match to OLX's settings".
- The options have no Gameplay tab: the match is set up in Local Play
  before each game (it remembers the last one), and the tab edited the
  same settings.
- The in-game HUD is an opaque bar along the bottom, as OLX 0.58's local
  game had: health and weapons on the left, the minimap in the centre,
  as tall as the bar, a log of kills, deaths and the match's messages on
  the right. The game is drawn above the bar, never under it, and nothing
  fades any more. The clock stays top centre, the keys top right.
- The game is laid out for 16:9 and shows every player the same area of
  the level (427 × 192 world pixels, OLX 0.58's height above its bar):
  a 16:9 window has no black strips, a narrower one black above and
  below, a wider one at the sides. The bar is a fifth of the height.
- The menus and the HUD fill the same share of the screen's height on
  every display (1280 × 720 looks as before; 1440 lines are twice the
  size); the UI scale is on top of that.
- The options' Video section has one Display list: a 1280 × 720,
  1600 × 900 or 1920 × 1080 window, or borderless fullscreen. The
  desktop-sized window is gone (its bottom sat under the taskbar); a saved
  "Desktop size" opens fullscreen.

### Added

- Online play from the menus: host a game, find one in the LAN list or
  join by address; a lobby with chat, ready marks, spectators, loadouts,
  kick and ban; matches that run with rollback netcode. See `docs/online.md`.
- The dedicated server `rl-server`: a map rotation, bots filling empty
  seats, admin chat commands, bans kept in a file, LAN announcement.
- `rl-game --net-sim <profile>`: feel a network (lan, good, typical, bad,
  awful) against bots in one process.
- The network overlay (F3): ping, input delay, rollback and loss.
- Online replays: every online match saves its replay, equal on every peer;
  a desync saves a bundle.
- The Keys tab sets the online chat key (T by default).
- The netcode core for online play (not yet playable from the game):
  rollback with the host ordering inputs, spectators, desync detection and
  repair, tested by whole online matches with bots over simulated networks
  (`rl-cli net-sim`).
- `rl-cli net-repro` reads a desync bundle and names the first tick and
  world field that differ.
- The simulation is checked to give identical results on Linux, in the
  browser's WebAssembly, on Windows and on arm64 Linux (`cargo xtask
  determinism`; the WebAssembly leg needs `sudo apt install nodejs`, the
  arm64 leg `sudo apt install qemu-user gcc-aarch64-linux-gnu`).
- `cargo xtask net-feel` measures how online play feels on five simulated
  network profiles (rollback depth, visible corrections, freezes,
  bandwidth) and fails `full` if it gets worse.
- On a window 21:9 or wider the health, weapons and log can sit beside
  the 16:9 picture instead of in the bar: Options → Video → HUD layout
  (Auto, Bottom bar, Side panels), or `--hud-layout`.
- `cargo xtask envelope`: the performance envelope (players, projectiles,
  effects, map size, art scale, resolution, full-screen shader passes) on
  this machine, on WSL and native Windows, against OLX; results in
  `fixtures/bench/envelope/`, report in `docs/perf/envelope.md`.
- `rl-cli gpu-bench` (a recorded match drawn on the GPU at several sizes,
  with GPU timestamps and a shader-headroom probe) and `rl-cli stress`
  options `--fx-cap`, `--build-every`, `--memory-ceiling-mib`, `--record`
  and generated levels (`--level gen-<w>x<h>-a<scale>-s<seed>`).
- `rl-game --frame-log`: a windowed run's frame times as RON.
- `cargo xtask windows`: `rl-cli` and `rl-game` cross-compiled for Windows
  and run from WSL.

### Added

- 309 more worm skins (Hotrod among them): the ones OpenLieroX 0.58 rc5's
  release ships that its repository lacks, bundled from
  `content/olx-0.58-rc5/skins/` (718 skins in all).

### Fixed

- Import reads OLX images that are Windows bitmaps under a `.png` name, as
  OLX did: OLX 0.58's release has two such skins (Teletubbie, capworm).

- On Wayland (WSLg), changing the window's size or fullscreen no longer
  leaves pieces of the old window frame on the screen: the window is made
  again at its new size.

### Known limitations

- **Graphics** are OLX's look, drawn sharp at any size; lighting, glow and
  richer effects are the 0.2 milestone.
- **Bot skill** is OLX's, with the aim-error floor; the difficulty redesign
  is a later milestone.
- **Game modes:** Deathmatch only, with no teams.
- **No level editor or map making yet.**
- **No browser version** (deferred; the simulation still builds for WASM).
- **No Classic HUD:** OLX's bitmap fonts and HUD images are not imported.

### The first playable build (2026-10-11, tag `v0.1.0-local-play`)

The first playable release: a local Deathmatch against OLX's bots, started
from menus, on any of the 4 bundled levels with either mod (spec §1.3,
milestone M7). It is a Rust reimplementation of OpenLieroX (OLX, commit
`d1ad953f`). Physics, movement and shooting reproduce OLX's own recorded
traces bit for bit. Where the entries above changed something, they win over
the notes below, which describe that build.

#### Added

- **Content import (M1).**
  - Importers for OLX's gamescript v7 mods (`script.lgs`), image `.lxl`
    levels, themes, skins, the core sprites and sounds, and `.wps`
    weapon-restriction presets.
  - Imported content becomes open, text-based packs (`rl-pack`): versioned
    RON plus PNG and OGG, with content hashes.
  - `cargo xtask fetch-olx-content` imports the pinned OLX content: Liero
    Factory, Castle Strike, Fossil Facility and Dirt Level; Liero v1.0
    (Classic) and Modern Warfare 1.0; the Dirt theme; all 409 skins.
  - The importers have unit and property tests, and the imported content
    has a corpus snapshot.
- **Fidelity oracle (M2).**
  - A patched OLX in Docker (`tools/olx-oracle`) records scenarios as
    per-frame traces.
  - `rl-oracle` compares them field by field.
  - `rl-cli scenario` replays them and reports the first divergence.
- **Simulation (M3, M4).** `rl-sim` is `no_std` and also builds for
  `wasm32-unknown-unknown`. It covers:
  - OLX's 100/84 Hz clocks;
  - worm movement and collision, and the ninja rope;
  - the projectile tree, every projectile action, shots and beams;
  - explosions, carving, dirt, damage, death and respawn, and bonuses;
  - glibc-exact `sinf`, `cosf` and `atan2f`.

  109 of the 112 recorded oracle scenarios match OLX bit for bit in every
  field. The other 3 are pinned to their first divergence (see Deviations).
- **Presentation (M5).**
  - `rl-scene` holds OLX's camera, skins, sprites, beams and layer order.
  - `rl-render` (wgpu) draws at art scale and upscales: whole multiples
    (nearest) or sharp bilinear. Frames are interpolated.
  - `rl-fx` holds OLX's 1024-slot cosmetic entities: blood, gibs and
    stains.
  - `rl-audio` (kira) mixes positional sound with OLX's OpenAL defaults.
  - `rl-input` has OLX's keyboard and gamepad defaults, latched per tick.
  - A sandbox in `rl-game`, a fixed 100 Hz loop and render goldens.
- **Deathmatch and bots (M6).**
  - OLX's Deathmatch rules and texts (`rl-modes`).
  - OLX's `CWormBot` ported in `rl-bots`: navigation, aim, weapons, rope,
    four difficulties, and a "chaos" personality that fires everything.
  - Replays (`.rlrp`, checksummed, refused on other content).
  - `rl-cli match` and `rl-cli replay` for headless bot matches, with
    invariants, determinism and coverage.
  - Golden match checksums, and a bot-match grid over both mods, 4 levels
    and 3 seeds in `cargo xtask full`.
- **Menus, HUD and v0.1 (M7).**
  - **`rl-ui`:** the menus' logic and egui screens, with settings and
    profiles saved as versioned RON in the platform data directory. A
    broken file is set aside as `.bak`; one from a newer version is left
    untouched, and the main menu says so.
  - **Main menu:** Local Play, Sandbox, Profiles, Options, Replays, Import,
    About, Quit.
  - **Local Play:**
    - the mod and the level, with thumbnails made at import;
    - OLX's Deathmatch rules (lives, kill and time limits, loading time,
      bonuses, restriction preset);
    - 1 to 31 bots at one difficulty (Easy, Medium, Hard, Xtreme) or each
      its own, with their personality and loadout;
    - the profile you play.
  - **Profiles:** name, colour and skin, with a live colorized preview of
    the walk animation.
  - **Options:**
    - window-size presets and fullscreen, VSync or uncapped, the upscale
      filter, interpolation and UI scale;
    - master and effects volume;
    - key and gamepad rebinding that never leaves an action unreachable;
    - the default match.
  - **OLX's weapon selection** before each match: five slots, Random,
    Done, the keyboard, a gamepad or the mouse.
  - **The in-game HUD:**
    - health and five weapon slots with reload bars;
    - kill feed, clock and kill limit;
    - a minimap in OLX's colours;
    - Tab's scoreboard;
    - the minimap and the health and weapons panel fade while a worm is
      under them.
  - **Pause menu:** Esc gives Resume, Options and Quit to menu. No tick
    runs while paused.
  - **Results:** OLX's columns plus score, deaths and suicides, with
    Rematch, Save replay and Back to menu.
  - **Replays:** every Deathmatch played from the menus is saved (the
    last 20), and saved ones are watched from the Replays screen.
  - **In-game import** of an OLX game directory on a background thread,
    layered over the bundled packs.
  - **About:** the credits and licenses.
  - **Fast-forward** for games nobody plays (replays and `--spectate`):
    1×, 2×, 4×, 8×, 16× and max, with `--speed` and PageUp/PageDown. The
    simulation is identical at every speed.
  - **`--autoplay <script.ron> [--fake-clock]`** drives the real app by
    widget label. `cargo xtask e2e` runs the menus end to end on Xvfb.
  - **Benchmarks** (divan): tick, bots, draw list and render frame.
  - **An engine stress test:** 32 chaos bots, OLX's projectile pool and a
    20 000 pool. Baselines are in `fixtures/bench/`, and a regression of
    more than 10 % fails `cargo xtask full`.
  - The exit summary gives frame and tick-time percentiles and the bots'
    think time.
- **Tooling.**
  - `cargo xtask check`: format, clippy, layering, the wasm build, content
    and tests, about 30 s.
  - `cargo xtask full`: the bot grid, the GPU, render and HUD goldens, the
    10-minute soak, the audio check, e2e, bench and stress.
  - `bless-render` and `bless-regression` record a reason with every
    re-recorded golden.
  - `tidy` reclaims disk space.

#### Deviations from OLX

The full list, with reasons, is in `docs/porting/STATUS.md`.

- **Exact-zero deaths (spec §18 D9).** A worm left at exactly 0 health dies
  as in OLX, but OLX's Gusanos death particles are not simulated. Their
  random draws shift later respawn spots, so 3 MW 1.0 oracle scenarios
  (`assassin-s-shot`, `flak`, `plasma-launcher`) diverge from their respawn
  frame on.
- **Bots never aim perfectly** (the user's direction). Every difficulty
  keeps at least 1/6 of the aim error, so Xtreme has half of Hard's error
  where OLX's has none. Bots think once per 100 Hz tick on a work budget,
  draw from their own random stream and press the same keys as a player.
- **The damage score is counted once.** OLX counts it twice; it only breaks
  score ties.
- **Modern presentation.**
  - The art is drawn at art scale, then upscaled to any window and
    interpolated (`--no-interp` gives OLX's whole-pixel drawing).
  - The HUD, names and menus are egui at the display's resolution. OLX's
    bitmap-font Classic HUD was dropped (the user's call).
  - The menus are laid out anew rather than copied from OLX's 640 × 480
    screens.
  - The minimap adds the camera's view rectangle.
  - The HUD lies over the playfield, where OLX keeps it outside: the
    minimap and the health and weapons panel fade to 25 % in 150 ms while
    a worm is under them, and the minimap is at most 1.5 × OLX's size.
- **Things OLX does not have:**
  - replays, fast-forward, `--spectate` and the engine stress test;
  - level thumbnails made at import;
  - profiles that keep Unicode names;
  - any number of keys per action.
- **Fixed window size.** The window is not drag-resizable or maximizable;
  sizes come from Options presets or `--fullscreen`, paced at most one
  every 250 ms (WSLg's compositor crashes on quick resizes).
- **OLX content is converted, not read in place.** Liero `.lev` levels,
  Gusanos mods and levels, and non-PNG skins are not imported.

#### Known limitations

- **No network play.** Its model, with no master server, is a later
  design.
- **Bot skill** is OLX's, with the aim-error floor. The planned
  difficulty redesign (strategy, reactions, predictive reloading,
  teamwork) is a later milestone, together with a tournament command.
- **Game modes:** Deathmatch only, with no teams.
- **Replays** have no seeking, and they draw every worm with the default
  skin and colours (the human's look is not recorded).
- **Spectating** a bot match is only possible from the command line
  (`--spectate`), not from the menus.
- **No Classic HUD:** OLX's bitmap fonts and HUD images are not imported
  (the user's call).
- **No level editor or map making yet.**
- **`--autoplay` in a real window** sometimes loses a click (3 of 14 in
  the acceptance runs, none on Xvfb); it is tooling only.
- Developed and tested on Linux (WSL2 with WSLg); other platforms are
  untested.
