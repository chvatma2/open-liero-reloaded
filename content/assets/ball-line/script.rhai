// The Dirt-ball line's default script (the plant works, 2026-11-04): while
// it runs (any state but Stopped) the stamp counts down on its LEDs (Lit 1
// to Lit 7, a tick each), drops its die onto the belt (killing a worm
// under it), thumps, rises again, and a dirt ball leaves it along the belt,
// through the drive and up the incline, falling into the crate at the
// top, where it vanishes. Stopped (the level script's shift, or a power
// cut) lifts the die and holds the balls where they are.
// Its constants (ticks are 10 ms):
fn STEP() { 40 }    // ticks each LED (7 of them: the countdown)
fn HOLD() { 20 }    // ticks all seven are lit before the drop
fn DROP() { 400 }   // the die's speed down, px a second (4 a tick)
fn PRESS() { 15 }   // ticks it stays down
fn RISE() { 60 }    // its speed up, px a second
fn BELT() { 12 }    // the belt's speed, px a second
fn LIFT() { 2 }     // a ball's speed up in the drive, onto the incline's foot
fn FALL() { 60 }    // a ball's speed falling into the crate
fn KILL() { 1000 }  // the damage the die deals a worm it wedges

// A ball's path, from its place leaving the stamp (each ball's own place
// is BASE() x (i - 1) px further along the flat run): into the drive
// (inside its casing, drawn over the balls), through it and up onto the
// incline's foot (the user's playtest, 2026-11-05: no jump), up to its
// top, over the end, down into the crate.
fn BASE() { 4 }
fn BALLS() { 6 }
fn IN() { 32 }
fn FOOT() { [66, -3] }
fn TOP() { [91, -28] }
fn OVER() { 96 }
fn RIM() { -20 }
// Names by number (the engine adds no number to a string).
fn KEY(i) { ["b1", "b2", "b3", "b4", "b5", "b6"][i - 1] }
fn LIT() { ["Lit 1", "Lit 2", "Lit 3", "Lit 4", "Lit 5", "Lit 6", "Lit 7"] }

// `mem.run` (running), `mem.lit` (LEDs lit), `mem.b<i>` each ball's leg
// of the path: 0 waiting in the stamp (hidden), 1 the flat run, 2 the
// drive, 3 the incline, 4 over the end, 5 falling.
fn start() {
  this.mem.run = false;
  this.mem.lit = 0;
  this.mem.cut = -1;
  for i in 1..=BALLS() { this.set_leg(i, 0); }
  if this.state() != "Stopped" { this.begin(); }
}

// Stopped and Lit 1 are the level script's orders. A countdown step of
// this script's own in the same tick may have overwritten Stopped: it is
// set again. Only Lit 1 starts the line, and not in the tick of a stop
// (this script's own Lit 1 overwriting it).
fn entered(s) {
  if s == "Stopped" {
    this.mem.cut = this.time();
    if this.mem.run { this.halt(); }
    if this.state() != "Stopped" { this.set_state("Stopped"); }
  } else if s == "Lit 1" && !this.mem.run && this.time() != this.mem.cut {
    this.begin();
  }
}

// Starts running: the countdown from where the LEDs are, the balls on.
fn begin() {
  this.mem.run = true;
  if this.mem.lit == 0 { this.mem.lit = 1; }
  for i in 1..=BALLS() { this.roll(i); }
  if this.get("die_y") > 0 { this.rise(); } else { this.after(STEP(), "count"); }
}

// Stops: the die back up, the balls held, the countdown dark.
fn halt() {
  this.mem.run = false;
  this.mem.lit = 0;
  this.cancel("count");
  this.cancel("drop");
  this.cancel("rise");
  this.move_to("die_y", 0, RISE());
  this.move_to("ram_y", 0, RISE());
  for i in 1..=BALLS() {
    this.stop(KEY(i) + "_x");
    this.stop(KEY(i) + "_y");
  }
}

// The next LED, or the drop after the seventh.
fn count() {
  if !this.mem.run { return; }
  let n = this.mem.lit + 1;
  if n > 7 { return; }
  this.mem.lit = n;
  this.set_state(LIT()[n - 1]);
  if n == 7 { this.after(HOLD(), "drop"); } else { this.after(STEP(), "count"); }
}

fn drop() {
  if !this.mem.run { return; }
  this.move_to("die_y", this.max("die_y"), DROP());
  this.move_to("ram_y", this.max("ram_y"), DROP());
}

fn rise() {
  this.move_to("die_y", 0, RISE());
  this.move_to("ram_y", 0, RISE());
}

fn arrived(name) {
  if name == "die_y" {
    if this.get("die_y") > 0 {
      // Down: a ball pressed, then up again.
      this.play("thump.wav");
      this.press();
      this.after(PRESS(), "rise");
    } else if this.mem.run {
      this.mem.lit = 1;
      this.set_state("Lit 1");
      this.after(STEP(), "count");
    }
    return;
  }
  for i in 1..=BALLS() {
    if name == KEY(i) + "_x" || (name == KEY(i) + "_y" && this.leg(i) == 5) {
      this.next(i);
      return;
    }
  }
}

// Something under the die stopped it (a worm wedged only holds it): up.
fn blocked(name) {
  if name == "die_y" { this.after(PRESS(), "rise"); }
}

// A worm the die wedges dies at once (the user's playtest, 2026-11-05):
// the press is no place to stand.
fn crushed(worm) { this.hurt(worm, KILL()); }

// A ball leaves the stamp: the first one waiting.
fn press() {
  for i in 1..=BALLS() {
    if this.leg(i) == 0 {
      let b = BASE() * (i - 1);
      this.set(KEY(i) + "_x", -b);
      this.set(KEY(i) + "_y", 0);
      this.set(KEY(i) + "_on", 1);
      this.set_leg(i, 1);
      this.roll(i);
      return;
    }
  }
}

// Ball `i` on to its next leg.
fn next(i) {
  let leg = this.leg(i);
  if leg == 5 {
    // Into the crate.
    this.set(KEY(i) + "_on", 0);
    this.set_leg(i, 0);
    return;
  }
  this.set_leg(i, leg + 1);
  this.roll(i);
}

// Ball `i` moving along its leg (nothing for one waiting, or while the
// line stands).
fn roll(i) {
  if !this.mem.run { return; }
  let k = KEY(i);
  let b = BASE() * (i - 1);
  let leg = this.leg(i);
  if leg == 1 {
    this.move_to(k + "_x", IN() - b, BELT());
  } else if leg == 2 {
    // Inside the casing: on to the foot, up its 3 px before it is out.
    this.move_to(k + "_x", FOOT()[0] - b, BELT());
    this.move_to(k + "_y", FOOT()[1], LIFT());
  } else if leg == 3 {
    // Up the incline, 45 degrees: each axis at the belt's speed / 1.4.
    this.move_to(k + "_x", TOP()[0] - b, BELT() * 10 / 14);
    this.move_to(k + "_y", TOP()[1], BELT() * 10 / 14);
  } else if leg == 4 {
    this.move_to(k + "_x", OVER() - b, BELT());
  } else if leg == 5 {
    this.move_to(k + "_y", RIM(), FALL());
  }
}

// Ball `i`'s leg, in memory as `b<i>`.
fn leg(i) { this.mem[KEY(i)] }
fn set_leg(i, v) {
  let m = this.mem;
  m[KEY(i)] = v;
  this.mem = m;
}
