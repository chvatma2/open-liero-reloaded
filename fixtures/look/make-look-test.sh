#!/usr/bin/env bash
# Builds fixtures/look/levels/look-test (plan 15a Task 14) with olr-cli's
# level and look commands: a glowing sky over a rock building whose roof has
# two windows, a flickering lava strip, two lamps inside, a dark ambient and
# a slight grade. Run from the repository root; OLR picks the binary.
set -euo pipefail
OLR=${OLR:-target/debug/olr-cli}
P=fixtures/look/levels/look-test
rm -rf "$P"
"$OLR" pack new level look-test --out "$P" --name "Look Test" --size 512x256
"$OLR" level look "$P" --ambient 203048:25 --saturation 110 --tint fff0f0
"$OLR" level group add "$P" sky --colour fff0d2 --intensity 100 --reach unlimited
"$OLR" level group add "$P" lava --intensity 150 --reach 96 --animation "Flicker(speed: 6, depth: 20)"
E() { "$OLR" level element "$@"; }
# Open air above the ground (y 200 down stays dirt).
E add-layer "$P" --material background --name Air
E draw "$P" air --rect 1,1,510,199 --colour 8a94a4
# The sky: glowing background along the top.
E add-layer "$P" --material background --name Sky
E draw "$P" sky --rect 1,1,510,50 --colour 6890c0
E set "$P" sky --glow 100 --glow-group sky
# A rock building, hollow, two windows in its roof.
E add-layer "$P" --material rock --name Building
E draw "$P" building --rect 150,70,200,130 --colour b0a898
E erase "$P" building --rect 160,80,180,120
E erase "$P" building --rect 200,70,20,10
E erase "$P" building --rect 280,70,20,10
# A lava strip (dirt) on the ground, left of the building.
E add-layer "$P" --material dirt --name Lava
E draw "$P" lava --rect 20,196,100,8 --colour e05010
E set "$P" lava --glow 100 --glow-group lava
# Two lamps inside: a warm steady one, a pulsing blue one.
"$OLR" level light add "$P" --at 200,150 --colour ffc080 --intensity 120 --reach 90
"$OLR" level light add "$P" --at 300,150 --colour 80a0ff --intensity 100 --reach 70 \
    --animation "Pulse(period_ms: 1000, low: 30, high: 100)"
"$OLR" pack check "$P"
