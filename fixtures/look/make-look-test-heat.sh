#!/usr/bin/env bash
# Builds fixtures/look/levels/look-test-heat (issue #40) with olr-cli's
# level, look and material commands: a dark cave whose explosions heat it.
# Stone rock above and below, a dirt mound on the floor, a wooden pillar, a
# metal plate and a brick back wall; the base's dirt is dirt, its back
# stone. Run from the repository root; OLR picks the binary.
set -euo pipefail
OLR=${OLR:-target/debug/olr-cli}
P=fixtures/look/levels/look-test-heat
rm -rf "$P"
"$OLR" pack new level look-test-heat --out "$P" --name "Look Test Heat" --size 512x256
"$OLR" level look "$P" --ambient 8090b0:35 --solid-floor 403020:10 --soak 3
"$OLR" level material add "$P" stone dirt wood metal brick
E() { "$OLR" level element "$@"; }
# The base (dirt, front and back): dirt in front, stone behind.
E surface "$P" base --rule 'dirt:*=dirt'
E surface "$P" base --back --rule '*=stone'
# The cave: open air before a brick wall.
E add-layer "$P" --material background --name Cave
E draw "$P" cave --rect 30,50,452,150 --colour 4a3a34
E set "$P" cave --surface brick
# Stone above and below.
E add-layer "$P" --material rock --name Ceiling
E draw "$P" ceiling --rect 0,0,512,50 --colour 6c6a66
E add-layer "$P" --material rock --name Floor
E draw "$P" floor --rect 0,200,512,56 --colour 6c6a66
E set "$P" ceiling --surface stone
E set "$P" floor --surface stone
# A dirt mound on the floor.
E add-layer "$P" --material dirt --name Mound
E draw "$P" mound --circle 140,206,34 --colour 7a5230
E set "$P" mound --surface dirt
# A wooden pillar and a metal plate.
E add-layer "$P" --material rock --name Pillar
E draw "$P" pillar --rect 240,50,14,150 --colour 8a5a2a
E set "$P" pillar --surface wood
E add-layer "$P" --material rock --name Plate
E draw "$P" plate --rect 330,130,90,10 --colour 8a96a6
E set "$P" plate --surface metal
"$OLR" level bake "$P"
"$OLR" pack check "$P"
