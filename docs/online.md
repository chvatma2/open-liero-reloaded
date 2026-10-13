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
`rl-game --join ADDR` does the same from the command line. You can also join
mid-match as a spectator, and see each player's key fingerprint (8
characters) in the Lobby.

Online play is IPv4 only for now: a name is joined at its IPv4 address,
and an IPv6 address (or a name with IPv6 addresses only) is refused with
"IPv6 is not supported yet".

## Dedicated server

```
rl-server --config server.ron [--content content/packs] [--data-dir <dir>]
```

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
  the data directory.

The data directory (the config file's own by default) holds
`server-identity.ron`, the bans file, `server.log` (rotated at 10 MiB:
`server.log.1` and `.2` keep the older lines) and `desyncs/` bundles.
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
