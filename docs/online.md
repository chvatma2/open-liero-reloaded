# Playing online

rust-liero plays over WebRTC data channels (TCP for the handshake, UDP for
the game), both on port **23400** by default. Everything below is the same
on Linux and Windows. Players behind ordinary home routers need no port
forwarding: see "Through routers".

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
says "via relays" instead of a ping: Join asks it through the relays (see
"Through routers"). "Block this host" hides a host's games for
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
is not the game you were invited to" otherwise). A code carries an
address only when players can reach the host directly: its router opened
the port, a public address is set (Options → Online, or the server's
`public_address`), or the machine has a public address of its own with
no NAT (a VPS or cloud VM: STUN sees its own address). Without one, the code joins through its relay hints
and the joiner's own relays (see "Through routers"); a game with neither
an address nor relays cannot be joined ("this game has no address, and
there are no relays to join it through").

**Visibility.** Host → "Who can join": **Anyone (Internet)** announces the
game on the relays and on the LAN; **This network** on the LAN only;
**Invite only** nowhere (share the code: the lobby says "Checking how
players can reach you…" for up to 5 s while the router and the STUN
servers are asked, so copy the code after that; then "This code joins
through the relays" when it carries no address). The game's choice
defaults to This network. A Public or Invite game asks its router to open
its port, asks STUN servers how its router behaves, and takes joins
through its relays; a Lan game does none of that. A server's
`visibility` is `Public` (the default), `Lan` or `Invite`; a Lan server
maps no port and takes no joins through the relays. The lobby says where
the game stands: "Checking how players can reach you…", "Announcing on
the Internet…", "Listed on the Internet", or why not ("Not listed: no
relays (set them in Options)", "Not listed: no relay reachable", "Not
listed: your public address setting is not reachable from the Internet":
a private, local or IPv6 public address is never announced on public
relays, so your LAN address does not leave; a server logs a warning and
is not announced either). A Public game without a public address is
listed all the same and joined through the relays. Under that line the
lobby shows how players reach the game (see "Through routers").

**Relays and STUN in Options.** Options → Online has Region, Public
address (only when you forwarded the port yourself; empty: the port your
router opens, else joins through the relays), "Open the game's port on
my router (UPnP/NAT-PMP/PCP)" (on by default), Relays (`wss://` or
`ws://` URLs, at most 32), STUN servers (`host:port`), TURN relays
(`user:pass@host:port`, your own, at most 8; see "Through routers") and
Blocked hosts. "Reset … to defaults" restores the shipped lists; a player
who never edits a list follows later default lists. Relay edits apply at
once: the game drops the relays it was connected to and connects to the
new list (a game you host announces there right away, takes joins there
and its invite names them); blocking and unblocking apply at once too.
STUN, port mapping and public address edits apply the next time you
host. A server sets `relays`, `stun`, `region`, `public_address`,
`port_mapping` and `allow_local` in its config (see
`fixtures/server/example.ron`); an empty `stun` asks no one.

**Your own relay.** Any Nostr relay that accepts kind 31470 and 31471
events and forwards kind 21470 (short-lived handshakes from keys it has
never seen), without payment or authentication, works. Run one, then list
its URL in Options → Online → Relays on every player's game, and in
`relays` in `server.ron`; an invite carries one such URL of at most 64
bytes, and a joiner asks the host through the code's relays before its
own. A relay that only the office can reach needs `ws://host:port` (or
`wss://`); when every relay in the list is on your own network,
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
`visibility: Lan` or `Invite` to keep it off). A hosted Public or Invite
game also stays connected to its relays to take joins. A join through the
relays is encrypted to the host's key and signed with a key made for that
join alone, so the relays cannot tell which player joins; they still see
the joiner's IP address. A TURN relay, when one is used, sees both
players' addresses and carries the match's traffic (encrypted, as every
connection is).

**Passwords.** A game's password is never sent: the joiner proves it
knows it, only to a host that has first proved its key, and the proof is
bound to the joiner's key and the connection, so it cannot be replayed
elsewhere.

**Getting through routers.** The list and the invite tell you where a
game is; "Through routers" says how a join then gets through the two
routers between you.

## Through routers

Two players behind ordinary home routers can play each other with no
manual port forwarding. A join finds a path on its own, and both sides
see which one it found.

### The paths

A join tries these, and the first that connects wins. The host's lobby
tags each member with its path, and the joiner's lobby shows it under
"Connection".

| path | tag | how | the joiner's line |
|---|---|---|---|
| direct | `direct` | the host's address is reachable as it is: the same LAN, a typed address, a forwarded port, a host with no NAT | "connected directly to 1.2.3.4:23400" |
| open port | `open port` | the host's router opened the game's port (UPnP, NAT-PMP or PCP) | "connected directly to …" (by the address in the code) or "connected through the host's open port …" |
| hole punching | `punched` | both sides send to each other's public address at once, through both routers | "connected by hole punching through both routers" |
| TURN relay | `relayed` | a volunteer relay carries the traffic: the last resort, about 32 KiB/s each way, ample for a match | "connected through the TURN relay 1.2.3.4:3478" |

**The join's order.**

- A typed address and a LAN game are joined directly, as before.
- A listed game or an invite with an address is tried directly first.
  When that link never opens (5 s), the join goes through the Nostr
  relays.
- A game without an address goes through the relays at once.

Through the relays, the joiner:

1. connects to the code's relays and then its own (8 at most);
2. looks for its own paths: its public address from the STUN servers,
   and a TURN relay (your own from Options first, else ones announced on
   the relays, your region and the least busy first; 2 at most);
3. sends the host an encrypted offer, again every 2 s;
4. connects on whatever path the two sides' candidates allow.

The waiting text names each step: "Reaching the game directly…",
"Connecting to the relays…", "Looking for paths through your router…",
"Asking the game through the relays…", "Connecting…".

**When hole punching works.** It works between most home routers. It
fails when one router gives every destination another port (a
"symmetric NAT", common on mobile networks and some ISPs' routers) and
the other side has no open port. Such a pair needs the host's open port
or a TURN relay. The project runs no TURN relay of its own: volunteers
run them (`rl-server --relay`, below), or you add your own in Options →
Online → TURN relays. Until someone runs one, such pairs cannot connect.

### What the host's lobby says

Under the invite code, a Public or Invite game shows a listing line (see
"Visibility") and then how players reach it:

| line | meaning |
|---|---|
| "Asking your router to open port 23400…" | waiting for the router's answer (at most about 4 s) |
| "Port 23400 is open on your router (UPnP): players join directly at 1.2.3.4:23400" | the router opened the port (or NAT-PMP, PCP); the invite and the listing carry this address |
| "Your router opened port 23400 (…), but its own address 100.64.1.2 is not public (double NAT or carrier-grade NAT): players connect by hole punching or a TURN relay" | a second router or the ISP sits in between; the opened port does not help |
| "Your router did not open port 23400 (…): players connect by hole punching or a TURN relay" | UPnP, NAT-PMP and PCP are off on the router, or it refused; the reason is in brackets |
| "Port mapping is off: players connect by hole punching or a TURN relay" | Options' "Open the game's port on my router" is off (or a server's `port_mapping: false`) |
| "No router: players reach you directly at 1.2.3.4:23400" | the machine has a public address with no NAT (a VPS or cloud VM): STUN sees its own address; the invite and the listing carry it |
| "Players join directly at 1.2.3.4:23400 (your public address setting)" | the Public address in Options (a server's `public_address`) |
| "Your router changes ports for every destination (symmetric NAT): hole punching will fail; players need the open port or a TURN relay" | see "When hole punching works" |
| "Taking joins through 5 of 8 relays" | joiners can ask the game through that many relays |
| "Connecting to the relays…" | not connected to any relay yet |
| "No relay reachable: only LAN and direct joins work" | every relay failed; they are retried |

An Invite game whose code has an address shows no listing line;
otherwise it says "This code joins through the relays", or, with no
relays either, "This code has no address and no relays: it joins nothing
(set relays in Options)".

Each join through the relays also leaves a line in the host's lobby
(at most one of each kind every 5 s; the next says "(N more since)"):

| line | meaning |
|---|---|
| "A player asked to join through the relays: answered" | the offer arrived and was answered; the two sides now look for a path |
| "A player asked to join through the relays: refused, busy with other joins" | 8 other joins were connecting |
| "A player asked to join through the relays: refused (another game version)" | or another reason in brackets |
| "A join through the relays was answered but did not connect within 15 s: no network path" | the joiner sees "no network path…" (below) |
| "Ignored a join through the relays dated 120 s ahead of this PC's clock: check both PCs' clocks" | (or behind) one of the two clocks is off by more than a minute: set both PCs' clocks automatically |

No line at all after the joiner's "Asking the game through the
relays…" means the offer never arrived: the two sides share no relay,
or the relays refused it.

A server logs the reach lines as `reach: …`, the join lines as
`relays: …`, its invite as `invite code: rlx1…` (with an address) or
`invite code (joins through the relays): rlx1…`, and each join as
`join: <name> (<fingerprint>) from <ip> via <tag> key <key>`.

`rl-game` prints on its console (stdout): when hosting, each new reach
line (`rl-game: reach: …`), each join line (`rl-game: relays: …`), each
relay it listens on as it connects or fails (`rl-game: relay <url>:
connected` or `… unreachable: …`) and each member's path (`rl-game:
join: <name> via <tag>`, then `rl-game: path: <name> now via <tag>` when
it changes); when joining, each step (`rl-game: Asking the game through
the relays…`) and the connection line (`rl-game: connected …`). The
invite code is not printed: copy it from the lobby.

### When a join fails

The joiner's Multiplayer screen says why:

| message | what to do |
|---|---|
| "could not reach any Nostr relay: check your Internet connection, or the relays in Options → Online" | no relay answered: check the connection, or put relays back (Reset relays to defaults) |
| "the relays refused this join's handshake (some take events only from keys they know): try other relays in Options → Online" | every relay refused it; add relays that take events from new keys |
| "the relays refused this join's handshake as invalid: is this PC's clock right? (set it automatically)" | a relay found the join's events invalid, most often dated too far from its clock: set this PC's clock automatically (the network check prints each relay's clock) |
| "the game did not answer through the relays (it may have closed, its relays may differ from yours, or one of the two PCs' clocks is off by more than a minute)" | is the game still hosted? Do you share a relay with the host? An invite names the host's relays; a listed game is found on yours. Are both PCs' clocks right (set automatically)? The host's lobby says "Ignored a join … dated …" when they are not |
| "the game is busy with other joins: try again in a moment" | the host is answering 8 other joins; try again |
| "no network path to the game: hole punching failed and no TURN relay could be used. The host can open port N (TCP and UDP) on their router (UPnP, or port forwarding) or use a dedicated server; anyone can run a TURN relay (rl-server --relay)" | see below |
| "no network path to the game, not even through the TURN relay …: try again, or ask the host to open port N on their router" | the relay was found but did not connect; try again, or open the port |
| "this game has no address, and there are no relays to join it through (set relays in Options → Online)" | put relays back in Options |

**"No network path".** Hole punching failed and no TURN relay was used.
Fix one of these:

1. On the host, let the router open the port: turn on UPnP (or NAT-PMP or
   PCP) in the router's settings, keep "Open the game's port on my
   router" on, and host again. The lobby should then say "Port 23400 is
   open on your router".
2. Or forward TCP **and** UDP 23400 on the host's router to the host
   (see "Open the port"), set the Public address in Options → Online, and
   host again.
3. Or host on a dedicated server with a public address.
4. Or get a TURN relay: someone with an open port runs `rl-server
   --relay`, or you add one you run in Options → Online → TURN relays.
5. Run `rl-server --nat-check` on both machines (below) to see which
   side is the problem.

### Windows Firewall

The first time `rl-game` (or `rl-server`) opens its port, Windows asks
whether to allow it on the network. Allow it on **private and public**
networks: Windows calls many home networks "public". Hole punching and
relays work without that permission, since Windows lets in the replies to
what the game sent out. An open port needs it: a joiner's connection to
the router's opened or forwarded port arrives unasked, and the firewall
drops it. If you once clicked Cancel, allow `rl-game` in Windows Security
→ Firewall & network protection → "Allow an app through firewall".

### Run a TURN relay

```
rl-server --config server.ron --relay
```

A relay lets players whose routers cannot meet play through it. It
announces itself on the config's Nostr relays (the defaults if the config
names none), and joiners find it there. It runs beside the server's game
(`visibility: Invite` keeps that game unlisted).

A relay reaches games hosted behind its own public address (the
server's own game, or a player hosting on its LAN) only through the
router: the router must send the relay's packets for its own public
address back inside (hairpin NAT, which most home routers do and some
do not). Without it, a relay helps only games hosted on other networks:
run it on a third network, or on the joiner's side. On a machine with
no NAT (a VPS), its own game is always reachable.

**Ports.** UDP **3478** (TURN) and UDP **49160-49191** (one per player
relayed) must reach the machine, on the same port numbers outside.
`port_mapping: true` (the default) asks the router to open each of them
exactly. Otherwise forward them by hand, set `public_address` to the
machine's public IPv4 address, and allow them through the firewall. A
machine with no NAT needs neither. Change them with `relay: Some((port:
3478, ports: (49160, 49191), capacity: 32, user_kib_s: 32,
allocations_per_ip: 4))`; `capacity` is at most the number of ports.

**What the log says:**

- `relay: waiting for the public address`
- `relay: port 3478 mapped (UPnP) at 1.2.3.4:3478`
- `relay: 32 of 32 relay ports mapped`
- `relay: TURN on port 3478, relay ports 49160-49191, announced as 1.2.3.4:3478`
- then `relay: N allocations` as players come and go.

It does not start with `relay: not announced: no public address (set
public_address, or enable port mapping)`, `relay: not started: the router
mapped none of its relay ports exactly` or `relay: not started: no Nostr
relays to announce it on`. A port the router would not open exactly is
named (`relay: port P not mapped: …`) and left out; the relay then takes
fewer players.

**Limits.** Each player's IP gets at most 32 KiB/s each way, 4
allocations and 20 requests a second, and an allocation lasts at most 6
hours, so the relay cannot serve as a free proxy. A full relay with 32
players carries about 1 MiB/s each way. It relays only to public
addresses, never into your own network (`allow_local: true` lifts that
for an office relay), and never to its own ports. Its credentials are public by design: anyone may
use it within those limits. It sees the IP addresses of the players it
relays.

### Check the network: `rl-server --nat-check`

```
rl-server --nat-check [--config server.ron]
```

It prints what stands between this machine and the players, then exits.
It uses the config's STUN servers and relays, or the defaults without
`--config`. It asks the public STUN servers and the Nostr relays, and
opens and closes one port on the router. For example:

```
nat-check: what stands between this machine and the players
base address: 192.168.1.20
gateway: 192.168.1.1
STUN stun.cloudflare.com:3478: 203.0.113.7:51234
STUN stun.l.google.com:19302: 203.0.113.7:51234
NAT: a NAT that keeps ports (hole punching works)
router: asking it to open UDP port 53211
router: port 53211 opened (UPnP) at 203.0.113.7:53211
router: the mapping is deleted
relay wss://relay.damus.io: connected, its clock agrees with this PC's
relay wss://nos.lol: connected, its clock 2 s ahead of this PC's
…
TURN notes: 1
TURN relay 198.51.100.4:3478 (Europe, 3 of 32 in use)
nat-check done
```

How to read it:

- **`NAT:`**
  - `no NAT`: the machine has a public address.
  - `a NAT that keeps ports (hole punching works)`: hole punching works.
  - `symmetric NAT`: hole punching fails unless the other side has an
    open port, so this side needs the open port or a TURN relay.
  - `not known yet`: fewer than two STUN servers answered.
- **`router:`**
  - `opened`: port mapping works.
  - `but its address … is not public`: a double NAT or carrier-grade
    NAT.
  - `not opened: …` or `no answer within 5 s`: UPnP, NAT-PMP and PCP are
    off or refused.
  - `deleting the mapping: the router did not confirm it`: the router
    may keep the test mapping until it expires.
- **`relay …:`** `connected` and how far its clock is from this PC's
  (from its `Date` header; nothing said when it sends none), or why not.
  A `clock: this PC's clock differs from a relay's by more than a
  minute …` line follows when one is: joins through the relays fail when
  two PCs' clocks differ by more than a minute.
- **`TURN notes: 0`**: no volunteer relay is announced, so a symmetric
  pair cannot connect.

### Two PCs across home routers

A playtest of the whole chain: two PCs on two different home networks
(for example, at two homes, or one on a phone's hotspot), with no ports
forwarded by hand. Use the release zips and play on Windows or native
Linux (not WSL2, see "WSL2 and Windows"). Both PCs' clocks must be right
(set automatically): joins through the relays fail when they differ by
more than a minute.

**Commands.** Unpack the zip, open a terminal in its folder
(`rust-liero-<version>`), and start the programs from there:

| | Linux (a terminal) | Windows (PowerShell) |
|---|---|---|
| the game, keeping its output | `./rl-game \| tee rl-game.txt` | `.\rl-game.exe \| Tee-Object -FilePath rl-game.txt` |
| the network check | `./rl-server --nat-check > nat-check-host.txt` | `.\rl-server.exe --nat-check > nat-check-host.txt` |

On the joining PC, name the check's file `nat-check-joiner.txt`. (In
cmd.exe, `rl-game.exe` and `rl-server.exe --nat-check > nat-check-host.txt`
work as written. Windows PowerShell 5.1 writes the `>` file as UTF-16:
harmless, but open it with an editor rather than `type`.)

1. **On the host PC**, start the game as above. Allow the firewall
   prompt on private and public networks. Multiplayer → Host a game,
   Who can join: **Invite only**, Start hosting.
2. **Wait** until the lobby's Invite section stops saying "Checking how
   players can reach you…" (at most 5 s), then **Copy** the invite code
   and send it to the other player by any chat.
3. **On the joining PC**, start the game the same way, then Multiplayer
   → Direct, paste the code into "Address or invite code" and Join. The
   waiting text shows each step.
4. **Read both lobbies**, and note what they say in the test report:
   - The host's Invite section, under the code: did the router open the
     port ("Port 23400 is open on your router (…)", or "No router: …")?
     Does it say the host's router is symmetric ("Your router changes
     ports for every destination (symmetric NAT): …")? How many relays
     take joins ("Taking joins through N of M relays")? (The joiner's
     router shows only in the joiner's network check, step 5.)
   - The host's chat log: "A player asked to join through the relays:
     answered", or the other join lines under "What the host's lobby
     says".
   - The host's member list: the joiner's tag beside its ping, one of
     `direct` / `open port` / `punched` / `relayed`.
   - The joiner's "Connection" section. It agrees with the tag, except
     that a join by the open port's address reads "connected directly to
     …".
5. **If the join fails**, note the joiner's message (the table under
   "When a join fails") and the host's join lines. Then run the network
   check on both PCs (the command above): its `NAT:` line is
   each router's kind, its `relay …:` lines each relay's clock against
   that PC's. Keep both files with both `rl-game.txt` outputs, then try
   the fixes under "No network path".

To exercise hole punching even when the host's router opens the port,
untick "Open the game's port on my router (UPnP/NAT-PMP/PCP)" in Options
→ Online on the host and host again: the lobby then says "Port mapping
is off: …", and the joiner's tag should be `punched`.

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
rl-server --config server.ron [--content content/packs] [--data-dir <dir>] [--relay]
rl-server --nat-check [--config server.ron]
```

Without `--content` the server uses the `content/packs` beside it (an
unpacked release zip), else the one in the working directory. `--relay`
also runs a volunteer TURN relay, and `--nat-check` checks the network
and exits (see "Through routers").

`fixtures/server/example.ron` documents every field:

- `name`, `visibility` (`Public`: announced on the Nostr relays and the
  LAN, the default; `Lan`: the LAN only; `Invite`: not announced; the old
  `DirectOnly` reads as `Invite`), `password`, `port` (TCP and UDP);
- `relays`, `stun`, `region`, `public_address` (see "Finding games"); a
  Public or Invite server also takes joins through its `relays`. A
  server on a VPS or cloud VM (a public address on its interface, no
  NAT) needs no `public_address`: STUN shows it has no NAT, and its
  invite and listing carry its own address (its log says "reach: No
  router: players reach you directly at …"). Keep `stun` set for it
  (an empty `stun` asks no one, and the server is then joined through
  the relays); its firewall must let TCP and UDP on `port` in;
- `port_mapping` (default `true`: a Public or Invite server asks the
  router to open its port, and with `--relay` the relay's ports; deleted
  at exit), `allow_local` (default `false`: accept private and local
  addresses where public ones are expected, for an office or LAN setup
  with its own relays) and `relay` (a TURN relay's ports and limits; see
  "Run a TURN relay");
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

Usually not needed: the game asks the router to open its port itself, and
hole punching or a TURN relay covers most of the rest (see "Through
routers"). Forward by hand when the router has UPnP, NAT-PMP and PCP off
and a joiner gets "no network path". Forward TCP **and** UDP 23400 (or
your `port`) to the host on the **same** external port, and allow them
through its firewall: a joiner sends its game traffic to the address and
port it dialled. Then set the Public address in Options → Online (a
server's `public_address`): a forwarded port is not found by itself, and
without it players join through the relays. This also holds for a server
in Docker: publish both protocols on one port. A cloud VM with a public
address of its own needs only its firewall opened (TCP and UDP on one
port): STUN finds its address.

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
