#!/usr/bin/env bash
# Builds fixtures/live/levels/live-test (issue #85) with olr-cli: the live
# assets in fixtures/live/assets/ (drawn in a scratch level, saved with
# `asset save`, then given states, values, health, a sound and a script)
# and a lit 320 × 200 level placing Live copies of them:
#   lamp       states On, Off and Dim (its bulb's glow), a background post,
#              a click on entering Off, a default script, its bulb's light
#              intensity exposed as lamp_light (issue #93);
#   console-a, console-b  one asset, states Normal and Alarm, its screen's
#              glow colour exposed as screen_colour: they start in
#              different colours, console-b stretched to twice its width;
#   press      a rock head whose height is exposed as head_y, on a rock frame;
#   crate      destructible (40 points, Wrecked at 0), of rock;
#   gate       a mixed layer (background with a rock bar) exposed as gate_x,
#              with its own script;
#   spot       a rock lamp with a directional background glow (its lens),
#              the lens's beam direction and spread exposed as beam_dir and
#              beam_spread (issue #93).
# Run from the repository root; OLR picks the binary.
set -euo pipefail
OLR=${OLR:-target/debug/olr-cli}
F=fixtures/live
P=$F/levels/live-test
A=$F/assets
W=$(mktemp -d)
trap 'rm -rf "$W"' EXIT
rm -rf "$P" "$A"
mkdir -p "$A"

# The workshop: every asset's layers, drawn apart.
WS=$W/ws
"$OLR" pack new level ws --out "$WS" --name Workshop --size 120x40
"$OLR" level look "$WS" --ambient 304060:30
"$OLR" level group add "$WS" lamp --colour ffd080 --intensity 300 --reach 40
"$OLR" level group add "$WS" screen --colour 40ff60 --intensity 200 --reach 24
"$OLR" level group add "$WS" spot --colour fff0c0 --intensity 1000 --reach 120
E() { "$OLR" level element "$@"; }
layer() { E add-layer "$WS" --material "$1" --name "$2"; }
# The lamp: a post and a glowing bulb.
layer background Post
E draw "$WS" post --rect 3,4,2,8 --colour 707880
layer rock Bulb
E draw "$WS" bulb --rect 2,0,4,4 --colour f0e080
E set "$WS" bulb --glow 80 --glow-group lamp
# The console: a case and a glowing screen.
layer rock Case
E draw "$WS" case --rect 10,0,12,10 --colour 404448
layer background Screen
E draw "$WS" screen --rect 12,2,8,5 --colour 203028
E set "$WS" screen --glow 70 --glow-group screen
# The press: a frame and a head.
layer rock Frame
E draw "$WS" frame --rect 26,0,10,2 --colour 606060
E draw "$WS" frame --rect 26,2,2,14 --colour 606060
E draw "$WS" frame --rect 34,2,2,14 --colour 606060
layer rock Head
E draw "$WS" head --rect 28,2,6,4 --colour a08040
# The crate: a box and its debris.
layer background Debris
E draw "$WS" debris --rect 40,6,8,2 --colour 6a4a2a
layer rock Box
E draw "$WS" box --rect 40,0,8,8 --colour 9a6a3a
# The gate: a background panel with a rock bar.
layer background Gate
E draw "$WS" gate --rect 52,0,12,12 --colour 304050
E draw "$WS" gate --rect 52,5,12,2 --colour 8090a0 --material rock
# The spot: a rock housing and a lens beaming from its right side.
layer rock Housing
E draw "$WS" housing --rect 70,0,6,4 --colour 505860
layer background Lens
E draw "$WS" lens --rect 76,1,1,2 --colour fff0c0
E set "$WS" lens --glow 100 --glow-group spot --glow-beam 0,20

save() { "$OLR" asset save "$WS" --elements "$1" --name "$2" --out "$A/$3"; }
save post,bulb Lamp lamp
save case,screen Console console
save frame,head Press press
save debris,box Crate crate
save gate Gate gate
save housing,lens Spot spot

# A click: 20 ms of a 1 kHz square wave, 8-bit mono WAV.
python3 - "$W/click.wav" <<'EOF'
import struct, sys
rate, n = 8000, 160
data = bytes(200 if (i // 4) % 2 else 56 for i in range(n))
fmt = struct.pack('<HHIIHH', 1, 1, rate, rate, 1, 8)
body = b'WAVE' + b'fmt ' + struct.pack('<I', len(fmt)) + fmt + b'data' + struct.pack('<I', n) + data
open(sys.argv[1], 'wb').write(b'RIFF' + struct.pack('<I', len(body)) + body)
EOF
printf 'fn on_enter(state) {}\n' > "$W/lamp.rhai"
printf 'fn tick() {}\n' > "$W/gate.rhai"

S() { "$OLR" asset state "$@"; }
# The lamp: On, Off and Dim.
S set "$A/lamp" Normal --name On
S add "$A/lamp" Off
S value "$A/lamp" Off bulb.glow 0
S add "$A/lamp" Dim
S value "$A/lamp" Dim bulb.glow 30
"$OLR" asset expose "$A/lamp" lamp_light bulb.light
"$OLR" asset sound add "$A/lamp" "$W/click.wav"
S set "$A/lamp" Off --enter-sound click.wav
"$OLR" asset script "$A/lamp" --set "$W/lamp.rhai"
# The console: Normal and Alarm, its screen's colour exposed.
"$OLR" asset expose "$A/console" screen_colour screen.glow-colour
S add "$A/console" Alarm
S value "$A/console" Alarm screen.glow-colour '#ff2020'
# The press: its head's height.
"$OLR" asset expose "$A/press" head_y head.y --default 0 --min 0 --max 6
# The crate: Normal hides the debris, Wrecked the box.
S add "$A/crate" Wrecked
S value "$A/crate" Normal debris.visible false
S value "$A/crate" Wrecked box.visible false
"$OLR" asset health "$A/crate" --points 40 --at-zero Wrecked
# The gate: its panel's offset.
"$OLR" asset expose "$A/gate" gate_x gate.x --default 0 --min -6 --max 6
# The spot: its lens's beam.
"$OLR" asset expose "$A/spot" beam_dir lens.beam-direction --default 45
"$OLR" asset expose "$A/spot" beam_spread lens.beam-spread --default 20
"$OLR" asset check "$A"/*

# The level: a lit cave with a rock floor.
"$OLR" pack new level olr-level-live-test --out "$P" --name "Live Test" --size 320x200
"$OLR" level look "$P" --ambient 304060:30
"$OLR" level carve "$P" --rect 8,8,304,160
add() { "$OLR" level copy add "$P" "$A/$1" --id "$2" --at "$3" "${@:4}"; }
set_() { "$OLR" level copy set "$P" "$@"; }
add lamp lamp 40,150
add console console-a 90,158
add console console-b 130,158 --size 24x10
add press press 190,152
add crate crate 230,160
add gate gate 270,156
add spot spot 150,40
set_ lamp --live --state Dim
set_ console-a --live --value 'screen_colour=#20ff40'
set_ console-b --live --value 'screen_colour=#4080ff' --state Alarm
set_ press --live --value head_y=3
set_ crate --live
set_ gate --live --value gate_x=-2
set_ spot --live
"$OLR" level copy script "$P" gate --set "$W/gate.rhai"
"$OLR" level bake "$P"
"$OLR" pack check "$P"
