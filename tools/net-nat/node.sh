#!/bin/sh
# node.sh <router lan ip> <command...>: a machine behind a home router
# (`cargo xtask net-nat`, tools/net-nat/CLAUDE.md). Its only way out is
# the router: the default route goes there, then the command runs.
set -eu
router=$1
shift
ip route replace default via "$router"
exec "$@"
