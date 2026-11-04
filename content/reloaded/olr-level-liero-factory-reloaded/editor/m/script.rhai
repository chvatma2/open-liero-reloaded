// The Mine cart's default script (the plant works, 2026-11-04): the cart
// rests at one end of its rails, then runs to the other (Right or Left,
// the lamp ahead lit, rattling), turning back where something stops it.
// Off (a power cut) stops it where it is; Stopped again sets it going.
// Its constants (ticks are 10 ms):
fn REST() { 300 }   // ticks it waits at each end (3 s)
fn SPEED() { 18 }   // px a second
fn WAIT() { 80 }    // ticks stopped by something before heading back

// `mem.right` is the way it goes next; `mem.off` whether the power is cut.
fn start() {
  this.mem.right = true;
  this.mem.off = false;
  this.mem.cut = -1;
  this.after(REST(), "run");
}

fn run() {
  if this.mem.off { return; }
  let s = if this.mem.right { "Right" } else { "Left" };
  let to = if this.mem.right { this.max("cart_x") } else { this.min("cart_x") };
  if this.state() != s { this.set_state(s); }
  this.move_to("cart_x", to, SPEED());
  this.move_to("lamp_x", to, SPEED());
  this.move_to("lamp_b_x", to, SPEED());
}

fn arrived(name) {
  if name != "cart_x" || this.mem.off { return; }
  this.set_state("Stopped");
  this.mem.right = !this.mem.right;
  this.after(REST(), "run");
}

// Stopped by rock (where the floor rises) or a part: the lamps back with
// the cart, a wait, then the other way.
fn blocked(name) {
  if name != "cart_x" { return; }
  this.lamps();
  this.set_state("Stopped");
  this.mem.right = !this.mem.right;
  this.after(WAIT(), "run");
}

fn lamps() {
  let x = this.get("cart_x");
  this.set("lamp_x", x);
  this.set("lamp_b_x", x);
}

fn entered(s) {
  if s == "Off" {
    this.mem.off = true;
    this.mem.cut = this.time();
    this.cancel("run");
    this.stop("cart_x");
    this.lamps();
    // A step of this script's own in the same tick may have overwritten it.
    if this.state() != "Off" { this.set_state("Off"); }
  } else if s == "Stopped" && this.mem.off && this.time() != this.mem.cut {
    this.mem.off = false;
    this.after(REST(), "run");
  }
}
