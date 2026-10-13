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

Multiplayer's LAN tab lists the games announced on your network and the
Internet tab those on the relays (see "Finding games"): pick one and Join.
For any other game type its address (`host` or `host:port`; a bare host
uses port 23400) or an invite code in the Direct tab and Join; a password is asked for when the game has one.
Three wrong passwords from one address within a minute hold that address
off for a minute ("too many wrong passwords: try again in a minute"),
everyone behind the same public address included.
`rl-game --join ADDR` (an address or an invite code) does the same from the command line. You can also join
mid-match as a spectator, and see each player's key fingerprint (8
characters) in the Lobby.

Online play is IPv4 only for now: a name is joined at its IPv4 address,
and an IPv6 address (or a name with IPv6 addresses only) is refused with
"IPv6 is not supported yet".

## Finding games

Multiplayer has three tabs.

**Internet.** The games announced on public Nostr relays (no master
server: hosts publish a signed note about their game and the game reads
the notes). The list shows each game's name, map, mod, players
(`(playing)` while a match runs), ping, region and a lock when it has a
password. Pick a game and Join; a click on a game measures its ping (the
time to connect to it, 2 s at most). A game of another version is greyed
out and says "needs version …". A game that announced no public address
says "cannot be joined yet". "Block this host" hides a host's games for
good (Options → Online lists the blocked hosts; Unblock there lists the
host again from its next announcement, within a minute). Above the list: "No relay
reachable" when none answers, "Searching…" until a relay has sent its
games. A game never gets listed on a note that is forged, old or aimed at
a private address.

**LAN** lists the games announced on your network. **Direct** takes an
address or an invite code.

**Invite codes.** A code starts with `rlx1` (letters and digits, in one
case). The host's lobby shows it with a Copy button, and a server prints
`invite code: rlx1…` in its log (and answers `/invite` in chat: anyone in a
Public game, admins otherwise). Send the code by any chat. The joiner pastes it into Direct (or
`rl-game --join rlx1…`) and Joins: the code holds the host's key, address
and a few relay hints, and the game only joins a host with that key ("this
is not the game you were invited to" otherwise). An invite without an
address means the host does not know its public address (no STUN answer, no
`public_address`): the code says which game, but the joiner gets "this
invite has no address: the game cannot be joined yet". Fix it on the host
(forward the port and set the public address in Options → Online, or the
server's `public_address`), then share the new code.

**Visibility.** Host → "Who can join": **Anyone (Internet)** announces the
game on the relays and on the LAN; **This network** on the LAN only;
**Invite only** nowhere (share the code: the lobby says "This code has no
address …" when it carries none, and "Finding the public address…" while
it is asked, so copy it after that). The game's choice defaults to
This network. To learn its public address for the announcement and the
invite, a Public or Invite game asks a STUN server (a Lan game never
does). A server's `visibility` is `Public` (the default), `Lan` or
`Invite` and it asks STUN in all three, since it prints an invite either
way. The lobby says where the game stands: "Finding the public
address…", "Announcing on the Internet…", "Listed on the Internet", or
why not ("Not listed: no relays", "no relay reachable", "no public
address (forward port N or set it in Options)", "your public address
setting is not reachable from the Internet": a private, local or IPv6
public address is never announced on public relays, so your LAN address
does not leave; a server logs a warning and is not announced either).

**Relays and STUN in Options.** Options → Online has Region, Public
address (empty: asked from STUN), Relays (`wss://` or `ws://` URLs, at
most 32), STUN servers (`host:port`) and Blocked hosts. "Reset … to
defaults" restores the shipped lists; a player who never edits a list
follows later default lists. Relay edits apply at once: the game drops
the relays it was connected to and connects to the new list (a game you
host announces there right away and its invite names them); blocking and
unblocking apply at once too. STUN and public address edits apply the
next time you host. A server sets `relays`, `stun`, `region` and
`public_address` in its config (see `fixtures/server/example.ron`); an empty
`stun` asks no one.

**Your own relay.** Any Nostr relay that accepts kind 31470 events,
without payment or authentication, works. Run one, then list its URL in
Options → Online → Relays on every player's game, and in `relays` in
`server.ron`; an invite carries one such URL of at most 64 bytes, for
later use (the joiner's game does not read it yet: NAT traversal, plan
12b, will). A relay that only the office can reach needs `ws://host:port`
(or `wss://`); when every relay in the list is on your own network,
named by `localhost` or a private IP address (`ws://192.168.1.2:7000`),
the game lists games at private addresses as well, which it refuses from
public relays. A relay named by a host name (`ws://relay.office.lan`)
counts as public, since the game cannot tell where it is without looking
it up: name an office relay by its IP address.

**Privacy.** Opening Multiplayer (the Internet tab is the first;
`rl-game --join` goes through it too) connects to the relays in Options, 8 public
relays by default, which then see your IP address; a hosted Public game
is announced there with its public address. Remove every relay in
Options → Online to keep the game off the relays entirely (the Internet
tab then says "No relay reachable"; LAN and Direct work as before). A
`rl-server` without `visibility` is Public: an older config without it
now announces the server on the default public relays (set
`visibility: Lan` or `Invite` to keep it off).

**Passwords.** A game's password is never sent: the joiner proves it
knows it, only to a host that has first proved its key, and the proof is
bound to the joiner's key and the connection, so it cannot be replayed
elsewhere.

**Still needs a forwarded port.** The list and the invite tell you where
a game is; they do not get through a router. Players outside your LAN
still need the port forwarded as in "Open the port" until plan 12b (NAT
traversal) lands.

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

- `name`, `visibility` (`Public`: announced on the Nostr relays and the
  LAN, the default; `Lan`: the LAN only; `Invite`: not announced; the old
  `DirectOnly` reads as `Invite`), `password`, `port` (TCP and UDP);
- `relays`, `stun`, `region`, `public_address` (see "Finding games");
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
through the forwarded host; STUN only learns your public address for the
list and the invite, and no relay carries game traffic yet.

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
