// The Crane's default script (issue #97): its claw rests at one end of
// the rail, then crosses to the other, humming (Moving) while it moves.
// Issue #99: its warning beacon, under the trolley, turns while it moves.
// Its constants (ticks are 10 ms; tuned in issue #100):
fn REST() { 200 }   // ticks the claw rests at each end (2 s)
fn SPEED() { 25 }   // world px a second (0.25 a tick; halved in issue #100)
fn WAIT() { 50 }    // ticks stopped by something before heading back

// The ends are the claw's limits, `max("claw_x")` and `min("claw_x")`,
// never numbers, so a stretched copy sweeps its stretched rail. The
// direction of travel is `mem.up` (towards the max), the max first.
fn start() {
  this.mem.up = true;
  this.mem.turn = 0;
  this.after(REST(), "cross");
}

// Heads for the end `mem.up` names, Moving.
fn cross() {
  if this.state() != "Moving" { this.set_state("Moving"); }
  let to = if this.mem.up { this.max("claw_x") } else { this.min("claw_x") };
  this.move_to("claw_x", to, SPEED());
}

// At an end: Idle, a rest, then the other end.
fn arrived(name) {
  this.set_state("Idle");
  this.mem.up = !this.mem.up;
  this.after(REST(), "cross");
}

// Stopped by rock, another part or the level's edge (a wedged worm only
// holds it, #90): still Moving, it waits, then heads back to the end it
// came from.
fn blocked(name) {
  this.mem.up = !this.mem.up;
  this.after(WAIT(), "cross");
}

// Issue #99, the warning beacon (issue #95's pick: two deep orange beams
// 180 degrees apart, level, half-angle 10, a turn every 3 s, the lamp
// flaring end-on). Its lenses are layers of their own, so they follow the
// claw's offset every tick. Level beams: one always points right (`lens`,
// 0 degrees), the other left (`lens-b`, 180), and only their spread, reach
// and light change as it turns, the same every half turn. `Idle` puts it
// out (its state values); while Moving `mem.turn` counts the turn's ticks
// (from 0 at the start, going on where it stopped at each crossing).
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
