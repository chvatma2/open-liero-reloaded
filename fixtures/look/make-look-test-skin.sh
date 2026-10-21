#!/usr/bin/env bash
# Builds issue #12's demo: fixtures/look/skins/look-test-skin (a
# double-resolution skin with a glowing visor and a light, from OLX's
# default skin, by make-look-test-skin.py) and
# fixtures/look/levels/look-test-skin-level (a dark room with a rock
# pillar, olr-cli's level and look commands). Run from the repository root;
# OLR picks the binary.
set -euo pipefail
OLR=${OLR:-target/debug/olr-cli}
OUT=${OUT:-fixtures/look}
S=$OUT/skins/look-test-skin
rm -rf "$S"
python3 -I fixtures/look/make-look-test-skin.py content/packs/olx-skin-default "$S"
"$OLR" pack check "$S"
P=$OUT/levels/look-test-skin-level
rm -rf "$P"
"$OLR" pack new level look-test-skin-level --out "$P" --name "Look Test Skin Level" --size 427x192
"$OLR" level carve "$P" --rect 8,8,411,176
"$OLR" level fill "$P" --material rock --rect 200,104,6,80 --colour 6a6460
"$OLR" level fill "$P" --material rock --rect 8,176,411,8 --colour 6a6460
"$OLR" level look "$P" --ambient 304060:6 --soak 4
