#!/usr/bin/env python3
"""Records one OLR oracle scenario in a dedicated OpenLieroX.

OLX runs this script with `-dedicated -script` and talks to it over the
line protocol of tests/headless/control/_olx_pipe.py: we print a console
command, OLX answers with ":arg" lines and a lone ".".

The scenario comes from the JSON file named by ORACLE_SCENARIO, written by
`cargo xtask oracle-record`:
  level, mod, seed, frames, settings {setvar name: value}, worms [{x, y}],
  inputs [[frame, worm index, key mask]] (OracleHooks.h's KEY_* bits:
  up 1 ... change 128, strafe 256, prev 512, next 1024, weapon1 2048 ...
  weapon5 32768),
  weapons [{worm, names, current}], health [[worm index, hp]],
  bonuses [[frame, x, y, "health"|"weapon", weapon name or ""]],
  out (trace path).
The whole run is driven by simStep, so no frame depends on the wall clock.
"""

import json
import os
import sys

# Every key bit OracleHooks.h declares (KEY_UP ... KEY_WEAPON5).
KEY_BITS = 65535


def emit(msg):
    sys.stderr.write(msg + "\n")
    sys.stderr.flush()


def command(cmd):
    sys.stdout.write(cmd + "\n")
    sys.stdout.flush()
    args = []
    while True:
        line = sys.stdin.readline()
        if line == "":
            raise SystemExit(0)
        line = line.rstrip("\n")
        if line == ".":
            return args
        if line.startswith(":"):
            args.append(line[1:])


def state():
    res = command("getGameState")
    return res[0] if res else "?"


def frame():
    return int(command("getFrame")[0])


def step_until(cond, limit, what):
    for _ in range(limit):
        if cond():
            return
        command("simStep 1")
    if not cond():
        emit("ORACLE_ERROR timeout waiting for " + what)
        raise SystemExit(1)


def main():
    sc = json.load(open(os.environ["ORACLE_SCENARIO"]))
    settings = {
        "GameOptions.Network.RegisterServer": 0,
        "GameOptions.Network.UseIpToCountry": 0,
        "GameOptions.GameInfo.GameType": "Death Match",
        "GameOptions.GameInfo.LevelName": sc["level"],
        "GameOptions.GameInfo.ModName": sc["mod"],
        "GameOptions.GameInfo.ImmediateStart": 1,
        "GameOptions.GameInfo.AllowEmptyGames": 1,
        "GameOptions.GameInfo.ForceRandomWeapons": 1,
        # LX56 aim speed and acceleration, independent of client options.
        "GameOptions.GameInfo.ForceLX56Aim": 1,
    }
    settings.update(sc.get("settings", {}))
    for key, value in settings.items():
        command('setvar %s "%s"' % (key, value))
    command("simSetSeed %d" % sc["seed"])
    command("simStep 0")  # manual stepping from here on
    command("startlobby 23400")
    command("addBots %d" % len(sc["worms"]))
    command("startgame")
    step_until(lambda: state() == "Playing", 2000, "Playing")
    ids = command("getWormList")
    if len(ids) != len(sc["worms"]):
        emit("ORACLE_ERROR expected %d worms, got %s" % (len(sc["worms"]), ids))
        raise SystemExit(1)
    schedule = os.path.join(os.path.dirname(sc["out"]), "inputs.txt")
    with open(schedule, "w") as fh:
        for frame_no, worm, keys in sc["inputs"]:
            if keys & ~KEY_BITS:
                emit("ORACLE_ERROR unknown key bits in %d at frame %d" % (keys, frame_no))
                raise SystemExit(1)
            fh.write("%d %s %d\n" % (frame_no, ids[worm], keys))
        for frame_no, x, y, kind, weapon in sc.get("bonuses", []):
            fh.write("%d bonus %d %d %s %s\n" % (frame_no, x, y, kind, weapon))
    command("simSetSeed %d" % sc["seed"])
    command("simLoadInputs " + schedule)  # also aligns the physics clock
    for wid, w in zip(ids, sc["worms"]):
        command('spawnWorm %s "%d,%d"' % (wid, w["x"], w["y"]))
    for wpns in sc.get("weapons", []):
        spec = ",".join(wpns["names"] + ["cur=%d" % wpns["current"]])
        command('simSetWeapons %s "%s"' % (ids[wpns["worm"]], spec))
    for worm, hp in sc.get("health", []):
        command("simSetHealth %s %r" % (ids[worm], float(hp)))
    command("simRecord " + sc["out"])
    target = frame() + sc["frames"]
    command("simStep %d" % sc["frames"])
    step_until(lambda: frame() >= target, 10, "last frame")
    command("simRecord off")
    emit("ORACLE_DONE frames=%d" % sc["frames"])
    command("quit")


main()
