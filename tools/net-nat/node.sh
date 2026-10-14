#!/bin/sh
# node.sh <router lan ip> <command...>: a machine behind a home router
# (`cargo xtask net-nat`, tools/net-nat/CLAUDE.md). Its only way out is
# the router: the default route goes there, then the command runs for at
# most RL_NAT_LIFE seconds (default 120; SIGTERM, then SIGKILL 5 s
# later), so a run that was interrupted leaves nothing running for long.
set -eu
router=$1
shift
ip route replace default via "$router"
exec timeout --preserve-status -k 5 "${RL_NAT_LIFE:-120}" "$@"
