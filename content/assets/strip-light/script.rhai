// The Strip light's default script (issue #98): a faulty fluorescent tube.
// It stays On a long while, then stutters (a burst of flickers, each Off a
// moment, then On again, the starter ticking), then now and then stays
// dark a while before a recovering burst, or glows Dim a while.
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
  this.after(this.between(ON_MIN(), ON_MAX()), "burst");
}

// A burst of flickers; from the dark it comes On first (recovering).
fn burst() {
  this.mem.left = this.between(FLICKERS_MIN(), FLICKERS_MAX());
  if this.state() == "Off" { this.back(); } else { this.flicker(); }
}

// One flicker: Off a moment.
fn flicker() {
  this.set_state("Off");
  this.mem.step = "off";
  this.after(this.between(OFF_MIN(), OFF_MAX()), "back");
}

// On again (entering On ticks); the next flicker, or the burst is over.
fn back() {
  this.set_state("On");
  this.mem.step = "back";
  this.mem.left = this.mem.left - 1;
  let next = if this.mem.left > 0 { "flicker" } else { "settle" };
  this.after(this.between(BACK_MIN(), BACK_MAX()), next);
}

// After a burst: dark a while, then a recovering burst; Dim a while; or On.
fn settle() {
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
  if this.state() != "On" { this.set_state("On"); }
  this.mem.step = "on";
  this.after(this.between(ON_MIN(), ON_MAX()), "burst");
}
