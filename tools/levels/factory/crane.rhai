// The Crane's default script (issue #97): its claw rests at one end of
// the rail, then crosses to the other, humming (Moving) while it moves.
// Its constants (ticks are 10 ms; tuned in issue #100):
fn REST() { 200 }   // ticks the claw rests at each end (2 s)
fn SPEED() { 50 }   // world px a second (0.5 a tick)
fn WAIT() { 50 }    // ticks stopped by something before heading back

// The ends are the claw's limits, `max("claw_x")` and `min("claw_x")`,
// never numbers, so a stretched copy sweeps its stretched rail. The
// direction of travel is `mem.up` (towards the max), the max first.
fn start() {
  this.mem.up = true;
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

// Stopped by rock, another part or a wedged worm: still Moving, it waits,
// then heads back to the end it came from.
fn blocked(name) {
  this.mem.up = !this.mem.up;
  this.after(WAIT(), "cross");
}
