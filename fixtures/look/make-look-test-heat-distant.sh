#!/usr/bin/env bash
# Builds fixtures/look/levels/look-test-heat-distant (issue #41) with
# olr-cli's level, look and material commands: the heat level's cave of
# stone open onto a near wooden palisade (a distant layer at depth 8, which
# explosions heat as slices of their ball) and a far night sky (depth 80,
# stone, beyond any explosion's reach: no heat). Run from the repository
# root; OLR picks the binary.
set -euo pipefail
OLR=${OLR:-target/debug/olr-cli}
P=fixtures/look/levels/look-test-heat-distant
rm -rf "$P"
"$OLR" pack new level look-test-heat-distant --out "$P" --name "Look Test Heat Distant" --size 512x256
"$OLR" level look "$P" --ambient 8090b0:35 --solid-floor 403020:10 --soak 3
"$OLR" level material add "$P" stone wood
E() { "$OLR" level element "$@"; }
# No base: where no element gives a back, the distant layers show.
E remove "$P" base
# The far sky, then the near palisade (each moves to the top of the
# distant layers).
E add-layer "$P" --material background --name Sky
E draw "$P" sky --rect 0,0,512,256 --colour 18223a
E set "$P" sky --distant 80 --surface stone
E add-layer "$P" --material background --name Palisade
E draw "$P" palisade --rect 0,30,512,190 --colour 6a4424
for x in 0 32 64 96 128 160 192 224 256 288 320 352 384 416 448 480; do
    E draw "$P" palisade --rect "$x,30,3,190" --colour 3e2814
done
E draw "$P" palisade --rect 0,90,512,4 --colour 3e2814
E draw "$P" palisade --rect 0,160,512,4 --colour 3e2814
E set "$P" palisade --distant 8 --surface wood
# Stone above and below.
E add-layer "$P" --material rock --name Ceiling
E draw "$P" ceiling --rect 0,0,512,50 --colour 6c6a66
E add-layer "$P" --material rock --name Floor
E draw "$P" floor --rect 0,200,512,56 --colour 6c6a66
E set "$P" ceiling --surface stone
E set "$P" floor --surface stone
"$OLR" level bake "$P"
"$OLR" pack check "$P"
