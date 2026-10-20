#!/usr/bin/env bash
# A check that passes only on a clean origin/main: the gate fails on a
# change while the probe of main itself passes.
[ "$(git rev-parse HEAD)" = "$(git rev-parse origin/main)" ]
