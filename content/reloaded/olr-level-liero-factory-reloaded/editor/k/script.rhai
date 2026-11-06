// The Crane's default script (issue #97; the production line design,
// 2026-11-06): its round. The claw goes over the mine's hole, opens its
// jaws, lowers down the shaft into the pile, closes them on a handful of
// dirt, rises, carries it right over the reservoir and opens: the dirt
// falls. Then back over the hole, the jaws open. The physics moves the
// dirt; the script only drives the parts. Humming (Moving) while it moves.
// Issue #99: its warning beacon, under the trolley, turns while it moves.
// The plant works (2026-11-04): Off (a power cut) stops it where it is,
// jaws and all; Idle again goes on with the step it was in.
//
// The claw is parts of their own, each moved by its own values in the
// same ticks at the same speed: the trolley (`claw_x`), the cable
// (`cable_x`, `cable_y`, drawn paid out and pulled up by DROP), the hub
// (`hub_x`, `hub_y`) and the jaws (`jaw_r_x`, `jaw_r_y`, `jaw_l_x`,
// `jaw_l_y`; a jaw's x is the claw's, OPEN outward when open). The values
// step in that order, so a closed claw only travels right (the right jaw
// leaves before the left one follows): it comes back with its jaws open.
// Its constants (ticks are 10 ms):
fn HOLE_X() { 9 }   // the claw's x over the shaft's middle
fn DUMP_X() { 75 }  // over the reservoir
fn DROP() { 130 }   // how far the claw lowers: the hub just above the pile
fn OPEN() { 9 }     // each jaw's travel, outward: wider than its inside
fn SPEED() { 25 }   // world px a second along the rail
fn LOWER() { 30 }   // world px a second down and up
fn JAW() { 10 }     // world px a second a jaw opens or closes
fn DUMP() { 100 }   // ticks it waits open over the reservoir
fn WAIT() { 50 }    // ticks stopped by rock before trying again

// The round's steps, in order (`mem.step`); a step is over when every
// value it moved arrived (`mem.left`), or was blocked where that counts.
fn STEPS() { ["hole", "open", "down", "close", "up", "dump", "release", "wait"] }

fn start() {
  this.mem.step = 0;
  this.mem.left = 0;
  this.mem.open = false;
  this.mem.off = false;
  this.mem.cut = -1;
  this.mem.turn = 0;
  this.after(WAIT(), "begin");
}

// Starts the step `mem.step` names: its motions, or its wait.
fn begin() {
  if this.mem.off { return; }
  let step = STEPS()[this.mem.step];
  this.mem.left = 0;
  if step == "wait" {
    if this.state() != "Idle" { this.set_state("Idle"); }
    this.after(DUMP(), "next");
    return;
  }
  if this.state() != "Moving" { this.set_state("Moving"); }
  if step == "hole" { this.travel(HOLE_X()); }
  else if step == "dump" { this.travel(DUMP_X()); }
  else if step == "open" { this.mem.open = true; this.jaws(); }
  else if step == "close" || step == "release" {
    this.mem.open = step == "release";
    this.jaws();
  }
  else if step == "down" { this.lower(DROP()); }
  else if step == "up" { this.lower(0); }
  if this.mem.left == 0 { this.next(); }
}

// The next step (the round goes on from the first).
fn next() {
  if this.mem.off { return; }
  this.mem.step = (this.mem.step + 1) % STEPS().len();
  this.begin();
}

// Moves value `name` to `to` at `speed`, counted in `mem.left`.
fn drive(name, to, speed) {
  if this.get(name) != to {
    this.move_to(name, to, speed);
    this.mem.left += 1;
  }
}

// The jaws' x, the claw at `x`.
fn jaws_at(x) {
  let o = if this.mem.open { OPEN() } else { 0 };
  this.drive("jaw_r_x", x + o, SPEED());
  this.drive("jaw_l_x", x - o, SPEED());
}

fn travel(x) {
  this.drive("claw_x", x, SPEED());
  this.drive("cable_x", x, SPEED());
  this.drive("hub_x", x, SPEED());
  this.jaws_at(x);
}

fn jaws() {
  let x = this.get("claw_x");
  let o = if this.mem.open { OPEN() } else { 0 };
  this.drive("jaw_r_x", x + o, JAW());
  this.drive("jaw_l_x", x - o, JAW());
}

fn lower(y) {
  this.drive("cable_y", y - DROP(), LOWER());
  this.drive("hub_y", y, LOWER());
  this.drive("jaw_r_y", y, LOWER());
  this.drive("jaw_l_y", y, LOWER());
}

// One of the step's values is there.
fn arrived(name) {
  if this.mem.off { return; }
  this.mem.left -= 1;
  if this.mem.left <= 0 { this.next(); }
}

// Stopped by rock or another part (a wedged worm only holds it, #90).
// Going down that is as far as it goes: the whole claw settles there, so
// its parts stay together (the others may be a pixel lower: they come
// back up to it). A jaw is as closed or as open as it gets; anything else
// waits and tries the step again.
fn blocked(name) {
  if this.mem.off { return; }
  let step = STEPS()[this.mem.step];
  let jaw = step == "open" || step == "close" || step == "release";
  if step == "down" {
    this.mem.left = 0;
    this.lower(this.get(name));
    if this.mem.left == 0 { this.next(); }
  } else if jaw {
    this.arrived(name);
  } else {
    this.cancel("begin");
    this.after(WAIT(), "begin");
  }
}

// Off and Idle are the level script's orders; a step of this script's own
// in the same tick may have overwritten Off: it is set again. Idle goes on
// with the step Off stopped (its motions again, from where they are).
fn entered(s) {
  if s == "Off" {
    this.mem.off = true;
    this.mem.cut = this.time();
    this.cancel("begin");
    this.cancel("next");
    for name in ["claw_x", "cable_x", "cable_y", "hub_x", "hub_y", "jaw_r_x", "jaw_r_y", "jaw_l_x", "jaw_l_y"] {
      this.stop(name);
    }
    if this.state() != "Off" { this.set_state("Off"); }
  } else if s == "Idle" && this.mem.off && this.time() != this.mem.cut {
    this.mem.off = false;
    this.after(WAIT(), "begin");
  }
}

// Issue #99, the warning beacon (issue #95's pick: two deep orange beams
// 180 degrees apart, level, half-angle 10, a turn every 3 s, the lamp
// flaring end-on). Its lenses are layers of their own, so they follow the
// claw's offset every tick. Level beams: one always points right (`lens`,
// 0 degrees), the other left (`lens-b`, 180), and only their spread, reach
// and light change as it turns, the same every half turn. `Idle` puts it
// out (its state values); while Moving `mem.turn` counts the turn's ticks
// (from 0 at the start, going on where it stopped at each step).
fn TURN() { 300 }   // ticks a turn (3 s)
fn PEAK() { 400 }   // the beams' intensity side-on

// The half turn's first quarter, from side-on (0) to end-on, every 3
// ticks (3.6 degrees) at phase phi = tick x 1.2 degrees, l = cos(phi):
// the apparent spread atan(sin a / sqrt(cos^2 a l^2 - sin^2 a (1 - l^2)))
// (89 where undefined), the reach 360 l and the light 400 w,
// w = smoothstep(sin a, sin a + 0.25, l), for a = 10 degrees (the design's
// "The spotlight's look (issue #95)").
fn SPREAD() { [10, 10, 10, 10, 10, 11, 11, 11, 11, 12, 12, 13, 14, 15, 16, 17, 19, 21, 24, 28, 34, 44, 68, 89, 89, 89] }
fn REACH() { [360, 359, 357, 354, 349, 342, 335, 326, 315, 304, 291, 277, 262, 246, 229, 212, 193, 173, 153, 133, 111, 90, 67, 45, 23, 1] }
fn LIGHT() { [400, 400, 400, 400, 400, 400, 400, 400, 400, 400, 400, 400, 400, 400, 400, 400, 400, 400, 400, 350, 225, 86, 3, 0, 0, 0] }

// Table `t` at quarter tick `q` (0 to 75), between its entries in integers.
fn at(t, q) {
  let i = q / 3;
  let f = q % 3;
  if f == 0 { t[i] } else { t[i] + (t[i + 1] - t[i]) * f / 3 }
}

fn tick() {
  let x = this.get("claw_x");
  if this.get("lens_x") != x {
    this.set("lens_x", x);
    this.set("lens_b_x", x);
  }
  if this.state() == "Moving" { this.turn(); }
}

// One tick of the beacon's turn.
fn turn() {
  let h = this.mem.turn % (TURN() / 2);
  this.mem.turn = (this.mem.turn + 1) % TURN();
  let q = if h > TURN() / 4 { TURN() / 2 - h } else { h };
  let spread = this.at(SPREAD(), q);
  let light = this.at(LIGHT(), q);
  this.set("beam_spread", spread);
  this.set("beam_spread_b", spread);
  this.set("reach", this.at(REACH(), q));
  this.set("light", light);
  this.set("halo_glow", (PEAK() - light) / 4);
  this.set("halo_light", PEAK() - light);
}
