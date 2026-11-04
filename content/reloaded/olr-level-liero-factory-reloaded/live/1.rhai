// The Strip light's default script (issue #98): a faulty fluorescent tube.
// It stays On a long while, then stutters (a burst of flickers, each Off a
// moment, then On again, the starter ticking), then now and then stays
// dark a while before a recovering burst, or glows Dim a while.
// The plant works (2026-11-04): Dark (a power cut) and Broken (its health
// gone) are left alone, the tube's timers doing nothing, until it is On
// again; set Off from outside while On or Dim (Liero Factory Reloaded's
// level script, for a blast near it), it bursts.
// Its constants (ticks are 10 ms; tuned in issue #100):
fn ON_MIN() { 300 }       // ticks On between bursts, at least
fn ON_MAX() { 1200 }      // and at most
fn FLICKERS_MIN() { 2 }   // flickers a burst, at least
fn FLICKERS_MAX() { 6 }   // and at most
fn OFF_MIN() { 3 }        // ticks a flicker is Off, at least
fn OFF_MAX() { 10 }       // and at most
fn BACK_MIN() { 2 }       // ticks On between flickers, at least
fn BACK_MAX() { 15 }      // and at most
fn DARK_MIN() { 100 }     // ticks it stays Off after a burst, at least
fn DARK_MAX() { 400 }     // and at most
fn DIM_MIN() { 200 }      // ticks it glows Dim after a burst, at least
fn DIM_MAX() { 600 }      // and at most
// After a burst, of 12 equal draws: 3 (1 in 4) stay dark, 2 (1 in 6) Dim,
// the other 7 On again.
fn DRAWS() { 12 }
fn DARK_DRAWS() { 3 }
fn DIM_DRAWS() { 2 }

// Every draw is the scripts' generator's (`this.rand`), drawn in run
// order, so copies of the tube never flicker together, and the same match
// seed gives the same flicker. `mem.step` is where the tube is (`on`,
// `off`, `back`, `dark`, `dim`), `mem.left` the burst's flickers to come.

// A whole number from `lo` to `hi`, both included.
fn between(lo, hi) { lo + this.rand(hi - lo + 1) }

// Starts On (its asset's starting state).
fn start() {
  this.mem.step = "on";
  this.mem.left = 0;
  this.mem.cut = -1;
  this.after(this.between(ON_MIN(), ON_MAX()), "burst");
}

// A burst of flickers; from the dark it comes On first (recovering).
fn burst() {
  if this.halted() { return; }
  this.mem.left = this.between(FLICKERS_MIN(), FLICKERS_MAX());
  if this.state() == "Off" { this.back(); } else { this.flicker(); }
}

// One flicker: Off a moment.
fn flicker() {
  if this.halted() { return; }
  this.set_state("Off");
  this.mem.step = "off";
  this.after(this.between(OFF_MIN(), OFF_MAX()), "back");
}

// On again (entering On ticks); the next flicker, or the burst is over.
fn back() {
  if this.halted() { return; }
  this.set_state("On");
  this.mem.step = "back";
  this.mem.left = this.mem.left - 1;
  let next = if this.mem.left > 0 { "flicker" } else { "settle" };
  this.after(this.between(BACK_MIN(), BACK_MAX()), next);
}

// After a burst: dark a while, then a recovering burst; Dim a while; or On.
fn settle() {
  if this.halted() { return; }
  let r = this.rand(DRAWS());
  if r < DARK_DRAWS() {
    this.set_state("Off");
    this.mem.step = "dark";
    this.after(this.between(DARK_MIN(), DARK_MAX()), "burst");
  } else if r < DARK_DRAWS() + DIM_DRAWS() {
    this.set_state("Dim");
    this.mem.step = "dim";
    this.after(this.between(DIM_MIN(), DIM_MAX()), "steady");
  } else {
    this.steady();
  }
}

// Steady On until the next burst.
fn steady() {
  if this.halted() { return; }
  if this.state() != "On" { this.set_state("On"); }
  this.mem.step = "on";
  this.after(this.between(ON_MIN(), ON_MAX()), "burst");
}

// Dark or Broken: its timers do nothing (`mem.step` "halt").
fn halted() {
  let s = this.state();
  if s == "Dark" || s == "Broken" { this.mem.step = "halt"; true } else { false }
}

// Dark and Broken come from outside (the level script, its health): a
// step of this script's own in the same tick may have overwritten them, so
// they are set again. On again after a power cut or a repair (not this
// script's own step overwriting the cut, in the same tick): steady until
// the next burst.
fn entered(s) {
  if s == "Dark" || s == "Broken" {
    this.mem.step = "halt";
    this.mem.cut = this.time();
    if this.state() != s { this.set_state(s); }
  } else if s == "On" && this.mem.step == "halt" && this.time() != this.mem.cut {
    this.steady();
  } else if s == "Off" && (this.mem.step == "on" || this.mem.step == "dim") {
    // Not this script's own Off (that sets the step with it): a burst,
    // from the dark.
    this.cancel("burst");
    this.cancel("steady");
    this.mem.left = this.between(FLICKERS_MIN(), FLICKERS_MAX());
    this.mem.step = "off";
    this.after(this.between(OFF_MIN(), OFF_MAX()), "back");
  }
}
