// The Forklift's default script (the plant works, 2026-11-04): it backs
// away from the crate stack (Reversing, beeping), waits, drives up to it
// again (Driving) and waits there, its beacon turning all the while.
// Its constants (ticks are 10 ms):
fn REST_MIN() { 200 }  // ticks it waits at an end, at least
fn REST_MAX() { 600 }  // and at most
fn SPEED() { 8 }       // px a second

fn start() {
  this.mem.back = true;
  this.after(this.rest(), "drive");
}

fn rest() { REST_MIN() + this.rand(REST_MAX() - REST_MIN() + 1) }

fn drive() {
  let s = if this.mem.back { "Reversing" } else { "Driving" };
  let to = if this.mem.back { this.max("fork_x") } else { this.min("fork_x") };
  this.set_state(s);
  this.move_to("fork_x", to, SPEED());
  this.move_to("light_x", to, SPEED());
}

fn arrived(name) {
  if name != "fork_x" { return; }
  this.set_state("Idle");
  this.mem.back = !this.mem.back;
  this.after(this.rest(), "drive");
}

// A worm in the way, or anything else: the light back with it, then off
// the other way after a while.
fn blocked(name) {
  if name != "fork_x" { return; }
  this.set("light_x", this.get("fork_x"));
  this.arrived(name);
}
