// Liero Factory Reloaded's sound strip lights (the plant works,
// 2026-11-04; their own script, the Strip light's faulty tube switched
// off): Steady (On without the buzz); set Off from outside (the level
// script, for a blast near it), it flickers a few times. Dark (a power
// cut) and Broken (its health gone) are left alone, and set again when a
// step of this script's own in the same tick overwrote them; On (from
// outside) is Steady again.
fn FLICKERS_MIN() { 1 }   // flickers, at least
fn FLICKERS_MAX() { 3 }   // and at most
fn OFF_MIN() { 3 }        // ticks a flicker is Off, at least
fn OFF_MAX() { 8 }        // and at most

fn between(lo, hi) { lo + this.rand(hi - lo + 1) }

// `mem.left` the flickers to come (above 0 while this script flickers it).
fn start() { this.mem.left = 0; }

fn entered(s) {
  if s == "Dark" || s == "Broken" {
    this.mem.left = 0;
    if this.state() != s { this.set_state(s); }
  } else if s == "On" {
    this.set_state("Steady");
  } else if s == "Off" && this.mem.left == 0 {
    this.mem.left = this.between(FLICKERS_MIN(), FLICKERS_MAX());
    this.after(this.between(OFF_MIN(), OFF_MAX()), "back");
  }
}

fn flicker() {
  if this.state() != "Steady" { this.mem.left = 0; return; }
  this.set_state("Off");
  this.after(this.between(OFF_MIN(), OFF_MAX()), "back");
}

fn back() {
  if this.state() != "Off" { this.mem.left = 0; return; }
  this.set_state("Steady");
  this.mem.left = this.mem.left - 1;
  if this.mem.left > 0 { this.after(this.between(OFF_MIN(), OFF_MAX()), "flicker"); }
}
