# Playing online

rust-liero plays over WebRTC data channels (TCP for the handshake, UDP for
the game), both on port **23400** by default. Everything below is the same
on Linux and Windows.

## Host from the menus

Multiplayer, then Host: name the game, set the player count and an optional
password, then Start hosting. The Lobby opens: pick the map and the rules
(only the host can), your weapons ("Random" or the weapon screen), Ready, and
chat with Enter after pressing the chat key (T; rebind it in Options, Keys,
"Chat"). The host can kick, ban, move players to spectators and "Force start"
(players not ready then spectate that match). `rl-game --host [NAME]`
opens the Lobby straight away; `--port` changes the port.

## Join

Multiplayer lists the games announced on your LAN: pick one and Join. For
any other game type its address (`host` or `host:port`; a bare host uses
port 23400) and Join; a password is asked for when the game has one.
Three wrong passwords from one address within a minute hold that address
off for a minute ("too many wrong passwords: try again in a minute"),
everyone behind the same public address included.
`rl-game --join ADDR` does the same from the command line. You can also join
mid-match as a spectator, and see each player's key fingerprint (8
characters) in the Lobby.

Online play is IPv4 only for now: a name is joined at its IPv4 address,
and an IPv6 address (or a name with IPv6 addresses only) is refused with
"IPv6 is not supported yet".

## Missing content

You need not have the host's packs. When a game uses a mod, level or
theme you lack, or a player wears a skin you lack, the Lobby lists it under
"Downloads" with its progress, and the game downloads it from the host.
Ready waits until the mod, level and theme are complete ("get the missing
content first"); skins never hold you back (one that does not arrive is
drawn as the default skin). If a match starts while you are still
downloading, you wait in the lobby, still connected, and watch or play
once your content is complete.

- **What travels.** The match's mod, level and the level's theme, and
  every player's skin, each identified by the hash of its content; packs
  in the game's own format (never OLX's original files). Skins are
  fetched through the host: the host takes a skin it lacks from its
  wearer and passes it on. Every download is checked against its hash and
  loaded like any other untrusted pack. A pack you have under the same
  name but with other content is left alone and only hidden for that
  match; your own packs win for skins.
- **Portable file names.** A pack's file names must be portable: a host's
  pack with a name Windows cannot store as itself (`C:`, `NUL.png`, a
  trailing dot, two names alike but for case) is refused, on every system.
- **The 50 MB prompt.** When more than 50 MB is still to download (a
  partial download's bytes not counted), the Lobby asks first: "This game
  needs N MB of content from the host.", with Download and Leave.
  Nothing downloads before you answer, and what you approved is not asked
  about again.
- **Resuming.** A download cut off (you left, the host left) continues
  where it stopped the next time a host lists the same pack.
- **Speed.** At most 2 MB/s to each player and 4 MB/s from a host in all
  (a server's `upload_kib_s`); 256 KB/s while a match runs, so the game
  keeps its bandwidth.
- **Where the cache lives.** The game keeps downloads in its download
  cache: `~/.cache/rust-liero/downloads` on Linux,
  `%LOCALAPPDATA%\rust-liero\cache\downloads` on Windows (with
  `--data-dir D`: `D/cache/downloads`). A second game running on the same
  machine uses `downloads-2` (up to `downloads-4`). The cache holds at
  most 1 GB; the packs used least recently go first. Replays of matches
  on downloaded content find their packs there.
- **Clearing it.** Options → Online → "Download cache: N MB", Clear (not
  while you are in an online game), or delete the directory while the
  game is closed.

## Dedicated server

```
rl-server --config server.ron [--content content/packs] [--data-dir <dir>]
```

Without `--content` the server uses the `content/packs` beside it (an
unpacked release zip), else the one in the working directory.

`fixtures/server/example.ron` documents every field:

- `name`, `visibility` (`Lan`: announced on the LAN, or `DirectOnly`),
  `password`, `port` (TCP and UDP);
- `max_players` (1 to 32), `max_spectators` (at most 16), `fill_bots` (bots
  fill empty seats up to this many), `min_players`, `start_when_ready`
  (otherwise a match starts 60 s after the lobby gathers; players not ready
  watch it);
- `rotation`: the maps in turn (`mod`, `level`, `time_limit`, `kill_limit`,
  `lives`, `bonuses`);
- `admins`: public keys (64 hex characters) allowed to use admin commands;
- `bans_file`: bans (keys and addresses) kept across restarts, relative to
  the data directory;
- `upload_kib_s`: how fast the server sends content to players who lack
  it, in KiB/s in all (default 4096, held to 64 to 65536; 256 KiB/s while
  a match runs).

The server sends its packs to players who lack them, and passes players'
skins on (see "Missing content"). The data directory (the config file's
own by default) holds `server-identity.ron`, the bans file, `server.log`
(rotated at 10 MiB: `server.log.1` and `.2` keep the older lines),
`desyncs/` bundles and `downloads/`, the skins it fetched from players
(clear it by deleting it while the server is stopped). Two servers on one
data directory cannot share `downloads/`: the second says so in its log
and runs without one (players' skins it lacks are drawn as the default).
A bot client, `rl-cli net-client --join ADDR --bot`, downloads what its
`--content` lacks into `--cache DIR` (a temporary directory by default).
Ctrl+C, SIGTERM or typing `quit` stops it cleanly.

**Your admin key.** The game shows only an 8-character fingerprint, but the
server's log names every joiner's full public key: join the server once,
find your join line in `server.log` (or the terminal), and paste the key into
`admins`. Restart the server.

**Chat commands.** Anyone: `/players`, `/help`. Admins: `/kick <name>`,
`/ban <name>` (the key), `/banip <name>` (the key and the address),
`/unban <name|key prefix|address>`, `/next` (end the match, or skip to the
next map) and `/settings <time_limit|kill_limit|lives|bonuses> <value>`
(the next maps' settings, for the rest of the run). Admins cannot be kicked
or banned. Bans are soft: a banned player can return with a new key.

## Open the port

Players outside your LAN need TCP **and** UDP 23400 (or your `port`)
forwarded to the host on the **same** external port, and allowed through
its firewall: a joiner sends its game traffic to the address and port it
dialled. This also holds for a server in Docker or on a cloud VM: publish
or open both protocols on one port. A player behind their own NAT connects
through the forwarded host; the game does not use STUN or relays yet.

## Testing a bad network

`rl-game --net-sim <profile>` hosts with three bots in the same process and
joins over a loopback link impaired as the profile: `lan`, `good`, `typical`,
`bad` or `awful`. Press **F3** in any online match for the network overlay
(ping, input delay, rollback, loss).

## Replays and desync bundles

Every online match saves its replay in the game's `replays/` directory (the
Replays screen plays it; every peer's copy is equal). A desync saves a
bundle in `desyncs/` of the game's data directory (`--data-dir`; the
platform's by default) or of the server's.

## WSL2 and Windows

WSL2's default NAT networking hides the Linux side from your LAN, so a host
or server run inside WSL2 is not reachable by other machines and does not
see LAN announcements. Either set `networkingMode=mirrored` in
`%UserProfile%\.wslconfig` under `[wsl2]` and run `wsl --shutdown`, or play
with the Windows build (`cargo xtask windows` cross-compiles `rl-cli` and
`rl-game`) for real games.
