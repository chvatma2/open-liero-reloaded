# tools/net-nat

The Docker NAT tests (plan 12b Task 11, networking spec §9.7, ruling N58):
home routers in containers between isolated LANs and a test WAN, and our
own host-built (dev) binaries joining across them. `cargo xtask net-nat
[--keep] [--offline] [--only <scenario>]...` runs them (`xtask/src/nat.rs`); `cargo xtask full`
runs them after `e2e`. It prints `skip net-nat (<why and the fix>)` and
passes without Docker, a Debian-like `/etc/os-release` or the content;
when this Docker cannot make macvlan networks (rootless Docker, no
`macvlan` module); and when the image cannot be built (N58): the base
cannot be pulled, apt cannot reach the mirrors, the release lacks a
package, or `--offline` (`full --offline`) and the image is not built
yet. A broken Dockerfile or script fails.

## Files

- `Dockerfile`: `FROM ${BASE}`, the host's own distribution
  (`<ID>:<VERSION_ID>` of `/etc/os-release`, so host-built binaries run
  unchanged; it must be Debian or derived from it, the packages come from
  apt), plus `iproute2`, `nftables` and `miniupnpd-nftables` (all in
  Ubuntu 26.04: miniupnpd 2.3.9, nftables backend, with NAT-PMP and PCP).
  The image is `olr-nat:<first 12 hex of SHA-256(these files ++ base)>`:
  an edit to any file here builds a new one, and the superseded `olr-nat`
  tags are pruned like the oracle's (`cargo xtask tidy` too). Its build is
  the only network use (the base image and the packages), done once.
- `router.sh <kind> <wan ip> <lan ip>`: finds the two interfaces by
  address, sets nftables (table `inet filter`: forward from the LAN out,
  replies back, miniupnpd's chains; postrouting `masquerade`, or
  `masquerade fully-random` for `symmetric`; input dropped from the WAN;
  for `hairpin`, the relay's ports forwarded to the LAN's `.30` from the
  WAN and the LAN (`dnat`, `ct status dnat accept`) and a LAN packet so
  forwarded back into the LAN sent from the WAN address (`snat`);
  for `pmp`, PCP requests dropped: `udp dport 5351 @th,64,8 2`, the
  version byte, 2 for PCP and 0 for NAT-PMP, as miniupnpd switches both
  together) and, for `upnp`, `pcp` and `pmp`, starts miniupnpd from
  `miniupnpd.conf.in`; prints `router ready: ...` once miniupnpd
  listens, then lives `OLR_NAT_LIFE` seconds (120; 3600 with `--keep`).
- `node.sh <router lan ip> <command...>`: `ip route replace default via
  <router>`, then execs the command (our binaries, in a LAN) under
  `timeout --preserve-status -k 5 $OLR_NAT_LIFE` (the exit status stays
  the binary's: a host stopped with SIGTERM exits 0).
- `miniupnpd.conf.in`: the WAN and LAN interfaces, `ext_ip` (the WAN
  address), `ext_allow_private_ipv4=yes` (203.0.113.0/24 is reserved to
  miniupnpd), `secure_mode=yes`, `allow 1024-65535 <the LAN> 1024-65535`
  then deny all; `enable_upnp` and `enable_pcp_pmp` per router.

## Topology

Every network is `docker network create -d macvlan --internal --label
olr-nat=<run> --subnet <subnet> olr-nat-<run>-<net>` (`Run::network_args`,
unit-tested; `<run>` is xtask's process id, in every container's and
network's name): Docker gives an internal macvlan network a dummy parent
interface of its own, so nothing on it reaches the host, the user's
router or the Internet, and no container has a default route but the one
`node.sh` sets. Not bridges: with `br_netfilter` (Docker turns it on),
Docker's rules for an internal bridge drop every bridged packet addressed
outside its subnet, which is all of a router's traffic.

| network | subnet | router (WAN, LAN) | NAT |
|---|---|---|---|
| `wan` | 203.0.113.0/24 | — | — |
| `a` | 10.1.0.0/24 | A 203.0.113.101, 10.1.0.2 | port-restricted cone (`masquerade`) |
| `b` | 10.2.0.0/24 | B 203.0.113.102, 10.2.0.2 | symmetric (`masquerade fully-random`) |
| `c` | 10.3.0.0/24 | C 203.0.113.103, 10.3.0.2 | cone + miniupnpd, UPnP-IGD only |
| `d` | 10.4.0.0/24 | D 203.0.113.104, 10.4.0.2 | cone + miniupnpd, NAT-PMP and PCP (PCP answers first) |
| `e` | 10.5.0.0/24 | E 203.0.113.105, 10.5.0.2 | cone + miniupnpd, PCP dropped: NAT-PMP only |
| `f` | 10.6.0.0/24 | F 203.0.113.106, 10.6.0.2 | cone + a manual forward of UDP 3478 and 49160-49191 to 10.6.0.30, hairpinned (`hairpin`: the LAN reaches them at the WAN address, and so forwarded its packets come from the WAN address) |

On the WAN: `infra` at 203.0.113.10 (`olr-cli nostr-relay --listen
0.0.0.0:7777` and `olr-cli stun-server --listen 0.0.0.0:3479`), `relay` at
.20 (`olr-server --relay`, Invite, `public_address` 203.0.113.20, STUN the
infra's, no port mapping; its TURN server's binding answers are the
second STUN server; in `relay-beside` it runs instead in LAN F at
10.6.0.30, `public_address` 203.0.113.106, behind F's forward), for
`direct`, `public` at .30 (Public, `public_address` 203.0.113.30), and
for `no-nat`, `open` at .40 (Invite, no address configured, no port
mapping: STUN sees its own address, N47-a). In a LAN, the host is `.10` and the joiner
`.20`. Every config (written to `target/net-nat/configs/`) and every
joiner uses relay `ws://203.0.113.10:7777`, STUN `203.0.113.10:3479` and
`203.0.113.20:3478` (the relay server: the first only), and `allow_local`
(N62). The binaries (`target/debug/olr-server` and `olr-cli` with
`nat-tools`: `e2e`'s dev builds, `e2e::build_binaries`, so `full` builds
them once and never flips release `olr-cli`'s features), `content/packs`
and the configs are mounted read-only at `/rl/bin`, `/rl/content` and
`/rl/config`. Routers run with `--cap-add NET_ADMIN --sysctl
net.ipv4.ip_forward=1`, nodes with `--cap-add NET_ADMIN`, everything
with `--init` (SIGTERM reaches our processes) and `-e OLR_NAT_LIFE`; our
binaries on the WAN run under the same `timeout` as `node.sh`'s.

**Lifetimes.** Every container ends by itself within `OLR_NAT_LIFE`
seconds (`LIFE`, 120: past the 60 s scenario bound and the host's stop),
so a run that was killed leaves nothing running for long (its stopped
containers and networks are removed by the next run). Ctrl+C (SIGINT,
SIGTERM, SIGHUP, through `ctrlc`) stops the waits, removes the run's
containers and networks and exits 130; a second Ctrl+C exits at once.
One run at a time: a machine-wide lock, `<temp>/olr-nat.lock` (the runs
share subnets, which Docker will not overlap), waited for at most 10
min; holding it, a run removes anything labelled `olr-nat` first.

Two details a real router has and the test needs:
- **Input dropped from the WAN.** A packet that reaches the router itself
  unasked is dropped in `input`, so its conntrack entry is never
  confirmed. Accepted (to a closed port), the entry would stay, and the
  host's later packet to that peer would be masqueraded to another port:
  hole punching between two cone routers failed that way.
- **One method per mapping router.** olr-net asks PCP, NAT-PMP and UPnP at
  once and keeps the first mapping; PCP answers long before UPnP's search
  ends, so a router offering all three would always show PCP. C offers
  UPnP alone, D NAT-PMP and PCP (PCP wins), E drops PCP so the client
  falls back to NAT-PMP (about 2 s later).

## Scenarios

Each starts its own containers (stage by stage: infra; relay, routers,
public; host; joiner), is bounded at 60 s (`migration`: 90 s), saves every container's
stdout and stderr to `target/net-nat/<scenario>/<role>.{out,err}`, and is
removed by its label (`docker rm -f`) even when it fails. A failure names
the scenario, what was missing and the last 40 lines of each container's
output.

| name | host | joiner | checks | time here |
|---|---|---|---|---|
| `direct` | `public` | LAN A, `--join 203.0.113.30:23400 --lobby-only` | exit 0, `path: direct`, host `via direct` | 4.1 s |
| `upnp` | Invite in LAN C, `port_mapping: true` | LAN A, `--invite <code> --lobby-only` | `reach: Port 23400 is open on your router (UPnP)`, the code has an address, `path: direct`, host `via open port` | 5.0 s |
| `pcp` | Invite in LAN D, `port_mapping: true` | LAN A, `--invite --lobby-only` | `(PCP)`, the code has an address, `path: direct`, `via open port` | 5.2 s |
| `nat-pmp` | Invite in LAN E, `port_mapping: true` | LAN A, `--invite --lobby-only` | `(NAT-PMP)`, the code has an address, `path: direct`, `via open port` | 6.8 s |
| `hole-punching` | Invite in LAN A, no mapping | LAN C, `--invite --lobby-only --no-nostr-turn` | the code has no address, `path: punched`, `via punched` | 6.2 s |
| `relay` | Invite in LAN A | LAN B (symmetric), `--invite --matches 1` (a 0.2-minute match with one server bot) | `path: relayed`, `match 1:`, `via relayed`, `match end`, the relay's `relay: <n ≥ 1> allocations` (`allocations`), both exit 0 (the host on SIGTERM) | 22.0 s |
| `no-relay` | Invite in LAN A | LAN B, `--invite --no-nostr-turn` | exit 1, stderr `no network path to the game` | 19.7 s |
| `finding` | Public in LAN A, no mapping (listed without an address) | LAN B, `--listed "nat finding" --lobby-only` | `path: relayed`, `via relayed` | 7.0 s |
| `nat-check` | `olr-server --nat-check` in LAN B | — | `NAT: symmetric NAT` | 5.4 s |
| `no-nat` | Invite on the WAN at .40, no NAT (N47-a) | LAN A, `--invite --lobby-only` | `reach: No router: players reach you directly at 203.0.113.40:23400`, the code has an address, `path: direct`, `via direct` | 4.3 s |
| `relay-beside` | Invite in LAN F, its TURN relay beside it at 10.6.0.30 (TR-5; the WAN relay not run) | LAN B (symmetric), `--invite --lobby-only` | `path: relayed`, the relay's allocation, the host's `via relayed` or `via punched` (below) | 9.3 s |
| `migration` | Invite in LAN A (`Config::Migrate`: one fill bot, `min_players` 2, a 0.5-minute match) | `succ` in LAN C, then `other` in LAN B (symmetric), both `--invite --matches 1` | host killed 8 s after both print `match starts:`; `succ` `migrated: hosting`, `other` `migrated: succ hosts` then `path: relayed`, both exit 0, equal `checksum` lines, the relay's allocation | 48.9 s |

`relay-beside` runs with `allow_local`, as every scenario must
(203.0.113.0/24 is not global), and `allow_local` lifted TR-1's refusal
of the relay's own address too: the scenario checks that a relay beside
the host carries a join through a hairpinning router (and that TR-5's
own-port refusal, kept with `allow_local`, does not get in the way), not
TR-5's difference, which `olr-net`'s `a_relay_reaches_hosts_at_its_public_ip_but_never_its_own_ports`
pins without `allow_local`. With `allow_local` the relay reaches the
host at its LAN address, so the host sees the joiner as a
peer-reflexive address of the relay's (10.6.0.30) and names the path
`punched`; through the hairpin (no `allow_local`) it would see the WAN
address, a port the hairpin chose.

`migration` (plan 12c Task 10, spec §3.7) is the only scenario with two
clients and a crash. The runner starts `succ` (role `joiner`) and waits
for its `path:` line (in the lobby), so it joined first and is the first
successor (B5); then `other` (role `other`). Once both stderrs show
`match starts:` it waits 8 s and `docker kill`s the host (silence, no
goodbye), then waits for both to exit (`crash_host`; the clients'
`--timeout` is 80 s). The verdict reads the clients' stdout: `succ`
`migrated: hosting`; `other` `migrated: succ hosts` followed by a new
`path: relayed` line (it reconnected to `succ` through the TURN relay,
behind its symmetric NAT); both exit 0; their `checksum <tick> <hex>`
lines equal (`checksum_difference`: the first differing tick, and none at
all is a difference); the relay logged an allocation. `succ` (cone, LAN
C) reaches the host punched; it is the standby that hosts on after the
host is gone, announced on the WAN relay.

A browser host is plan 13's (ruling PL12b-3).

Times measured here (WSL2, Docker 29.1.3, 2026-10-14): the nine without `migration` about
84 s once built (the dev binaries); `migration` takes 48.9 s, so the twelve take about 146 s; the image 196 MB (51 MB content),
built in 12 s from a pulled base. `no-nat` and `relay-beside` (the final
review's fix pass, 2026-10-15) 4.3 s and 9.3 s.

## By hand

- `--keep` leaves the networks and the last scenario's containers (for
  an hour): `docker exec -it olr-nat-<run>-router-a nft list ruleset`,
  `docker logs olr-nat-<run>-host`. The next run removes them (and
  anything an interrupted run left) by the label `olr-nat` first.
- `--only relay --only finding` runs those (in table order).
