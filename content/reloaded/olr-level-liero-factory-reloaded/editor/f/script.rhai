// The Lift's default script (the plant works, 2026-11-04): the cage rests
// at one end of its shaft, then rides to the other, whining and blinking
// (Moving) on the way. Off (a power cut) stops it where it is; Idle again
// sets it going.
// Its constants (ticks are 10 ms):
fn REST() { 400 }   // ticks it waits at each end (4 s)
fn SPEED() { 20 }   // px a second
fn WAIT() { 50 }    // ticks stopped by something before heading back

// `mem.down` is the way it goes next; `mem.off` whether the power is cut.
fn start() {
  this.mem.down = true;
  this.mem.off = false;
  this.mem.cut = -1;
  this.after(REST(), "ride");
}

fn ride() {
  if this.mem.off { return; }
  if this.state() != "Moving" { this.set_state("Moving"); }
  let to = if this.mem.down { this.max("floor_y") } else { this.min("floor_y") };
  this.move_to("floor_y", to, SPEED());
  this.move_to("cage_y", to, SPEED());
}

fn arrived(name) {
  if name != "floor_y" || this.mem.off { return; }
  this.set_state("Idle");
  this.mem.down = !this.mem.down;
  this.after(REST(), "ride");
}

// Stopped by rock or another part (a wedged worm only holds it): the
// frame back with the floor, a wait, then back where it came from.
fn blocked(name) {
  if name != "floor_y" { return; }
  this.stop("cage_y");
  this.set("cage_y", this.get("floor_y"));
  this.mem.down = !this.mem.down;
  this.after(WAIT(), "ride");
}

fn entered(s) {
  if s == "Off" {
    this.mem.off = true;
    this.mem.cut = this.time();
    this.cancel("ride");
    this.stop("floor_y");
    this.stop("cage_y");
    this.set("cage_y", this.get("floor_y"));
    // A step of this script's own in the same tick may have overwritten it.
    if this.state() != "Off" { this.set_state("Off"); }
  } else if s == "Idle" && this.mem.off && this.time() != this.mem.cut {
    this.mem.off = false;
    this.after(REST(), "ride");
  }
}
