# tools/net-nat

The Docker NAT tests (plan 12b Task 11, networking spec §9.7, ruling N58):
home routers in containers between isolated LANs and a test WAN, and our
own release binaries joining across them. `cargo xtask net-nat [--keep]
[--only <scenario>]...` runs them (`xtask/src/nat.rs`); `cargo xtask full`
runs them after `e2e`. Without Docker, a readable `/etc/os-release` or the
content, it prints `skip net-nat (<why and the fix>)` and passes.

## Files

- `Dockerfile`: `FROM ${BASE}`, the host's own distribution
  (`<ID>:<VERSION_ID>` of `/etc/os-release`, so host-built binaries run
  unchanged; it must be Debian or derived from it, the packages come from
  apt), plus `iproute2`, `nftables` and `miniupnpd-nftables` (all in
  Ubuntu 26.04: miniupnpd 2.3.9, nftables backend, with NAT-PMP and PCP).
  The image is `rl-nat:<first 12 hex of SHA-256(these files ++ base)>`:
  an edit to any file here builds a new one, and the superseded `rl-nat`
  tags are pruned like the oracle's (`cargo xtask tidy` too). Its build is
  the only network use (the base image and the packages), done once.
- `router.sh <kind> <wan ip> <lan ip>`: finds the two interfaces by
  address, sets nftables (table `inet filter`: forward from the LAN out,
  replies back, miniupnpd's chains; postrouting `masquerade`, or
  `masquerade fully-random` for `symmetric`; input dropped from the WAN)
  and, for `upnp` and `pcp`, starts miniupnpd from `miniupnpd.conf.in`;
  prints `router ready: ...` once miniupnpd listens.
- `node.sh <router lan ip> <command...>`: `ip route replace default via
  <router>`, then execs the command (our binaries, in a LAN).
- `miniupnpd.conf.in`: the WAN and LAN interfaces, `ext_ip` (the WAN
  address), `ext_allow_private_ipv4=yes` (203.0.113.0/24 is reserved to
  miniupnpd), `secure_mode=yes`, `allow 1024-65535 <the LAN> 1024-65535`
  then deny all; `enable_upnp` and `enable_pcp_pmp` per router.

## Topology

Every network is `docker network create -d macvlan --internal --label
rl-nat`: Docker gives an internal macvlan network a dummy parent
interface of its own, so nothing on it reaches the host, the user's
router or the Internet, and no container has a default route but the one
`node.sh` sets. Not bridges: with `br_netfilter` (Docker turns it on),
Docker's rules for an internal bridge drop every bridged packet addressed
outside its subnet, which is all of a router's traffic.

| network | subnet | router (WAN, LAN) | NAT |
|---|---|---|---|
| `rl-nat-wan` | 203.0.113.0/24 | — | — |
| `rl-nat-a` | 10.1.0.0/24 | A 203.0.113.101, 10.1.0.2 | port-restricted cone (`masquerade`) |
| `rl-nat-b` | 10.2.0.0/24 | B 203.0.113.102, 10.2.0.2 | symmetric (`masquerade fully-random`) |
| `rl-nat-c` | 10.3.0.0/24 | C 203.0.113.103, 10.3.0.2 | cone + miniupnpd, UPnP-IGD only |
| `rl-nat-d` | 10.4.0.0/24 | D 203.0.113.104, 10.4.0.2 | cone + miniupnpd, NAT-PMP and PCP only |

On the WAN: `infra` at 203.0.113.10 (`rl-cli nostr-relay --listen
0.0.0.0:7777` and `rl-cli stun-server --listen 0.0.0.0:3479`), `relay` at
.20 (`rl-server --relay`, Invite, `public_address` 203.0.113.20, STUN the
infra's, no port mapping; its TURN server's binding answers are the
second STUN server) and, for `direct`, `public` at .30 (Public,
`public_address` 203.0.113.30). In a LAN, the host is `.10` and the joiner
`.20`. Every config (written to `target/net-nat/configs/`) and every
joiner uses relay `ws://203.0.113.10:7777`, STUN `203.0.113.10:3479` and
`203.0.113.20:3478` (the relay server: the first only), and `allow_local`
(N62). The binaries (`target/release/rl-server`, `rl-cli` with
`nat-tools`), `content/packs` and the configs are mounted read-only at
`/rl/bin`, `/rl/content` and `/rl/config`. Routers run with `--cap-add
NET_ADMIN --sysctl net.ipv4.ip_forward=1`, nodes with `--cap-add
NET_ADMIN`, everything with `--init` (SIGTERM reaches our processes).

Two details a real router has and the test needs:
- **Input dropped from the WAN.** A packet that reaches the router itself
  unasked is dropped in `input`, so its conntrack entry is never
  confirmed. Accepted (to a closed port), the entry would stay, and the
  host's later packet to that peer would be masqueraded to another port:
  hole punching between two cone routers failed that way.
- **One method per mapping router.** rl-net asks PCP, NAT-PMP and UPnP at
  once and keeps the first mapping; PCP answers long before UPnP's search
  ends, so a router offering all three would always show PCP. C offers
  UPnP alone, D NAT-PMP and PCP.

## Scenarios

Each starts its own containers (stage by stage: infra; relay, routers,
public; host; joiner), is bounded at 60 s, saves every container's
stdout and stderr to `target/net-nat/<scenario>/<role>.{out,err}`, and is
removed by its label (`docker rm -f`) even when it fails. A failure names
the scenario, what was missing and the last 40 lines of each container's
output.

| name | host | joiner | checks | time here |
|---|---|---|---|---|
| `direct` | `public` | LAN A, `--join 203.0.113.30:23400 --lobby-only` | exit 0, `path: direct`, host `via direct` | 4.1 s |
| `upnp` | Invite in LAN C, `port_mapping: true` | LAN A, `--invite <code> --lobby-only` | `reach: Port 23400 is open on your router (UPnP)`, the code has an address, `path: direct`, host `via open port` | 5.0 s |
| `pcp` | Invite in LAN D, `port_mapping: true` | LAN A, `--invite --lobby-only` | `(PCP)` or `(NAT-PMP)`, the code has an address, `path: direct`, `via open port` | 5.1 s |
| `hole-punching` | Invite in LAN A, no mapping | LAN C, `--invite --lobby-only --no-nostr-turn` | the code has no address, `path: punched`, `via punched` | 6.2 s |
| `relay` | Invite in LAN A | LAN B (symmetric), `--invite --matches 1` (a 0.2-minute match with one server bot) | `path: relayed`, `match 1:`, `via relayed`, `match end`, the relay's `relay: 1 allocations`, both exit 0 (the host on SIGTERM) | 22.1 s |
| `no-relay` | Invite in LAN A | LAN B, `--invite --no-nostr-turn` | exit 1, stderr `no network path to the game` | 19.7 s |
| `finding` | Public in LAN A, no mapping (listed without an address) | LAN B, `--listed "nat finding" --lobby-only` | `path: relayed`, `via relayed` | 7.0 s |
| `nat-check` | `rl-server --nat-check` in LAN B | — | `NAT: symmetric NAT` | 5.4 s |

A browser host is plan 13's (ruling PL12b-3).

Times measured here (WSL2, Docker 29.1.3, 2026-10-14): all eight about
75 s once built; the image 196 MB (51 MB content), built in 12 s from a
pulled base; the two release builds about a minute when `full`'s other
steps rebuilt `rl-cli` without `nat-tools`.

## By hand

- `--keep` leaves the networks and the last scenario's containers:
  `docker exec -it rl-nat-router-a nft list ruleset`, `docker logs
  rl-nat-host`. The next run removes them (and anything an interrupted
  run left) by the label `rl-nat` first.
- `--only relay --only finding` runs those (in table order).
