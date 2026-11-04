// Liero Factory Reloaded's level script.
// Issue #97: the terminal left of the crane (issue #100; the console under
// the deck before it) is in Alarm while the crane moves, or while the
// plant's alarm sounds, Normal otherwise, set only on a change.
// The plant works (2026-11-04):
// - The shift: the line stands QUIET() ticks, the horn sounds, and
//   HORN() ticks later it runs (the stamp counting down, the belt white)
//   for RUN() ticks; and again.
// - The alarm: a blast of at least BIG() damage, or KILLS() kills within
//   KILL_SPAN() ticks, sounds the roof's sirens for ALARM() ticks and
//   tints the plant's lamps red (the level's groups and every strip's
//   colour); then none for REARM() ticks.
// - The blackout: while the generator is wrecked the plant's lamps are
//   out (the level's groups at 0, the strips Dark), the line, the crane,
//   the lift and the mine cart stop, and the red emergency lamps light;
//   all come back when it runs again (its own script repairs it).
// - Maintenance: every MAINTAIN() ticks one broken strip is mended.
// - A blast within NEAR() px of a lit strip flickers it (set Off: its own
//   script bursts).
// Its constants (ticks are 10 ms):
fn QUIET() { 1500 }     // ticks the line stands (15 s)
fn HORN() { 150 }       // ticks from the horn to the line running
fn RUN() { 3000 }       // ticks it runs (30 s)
fn BIG() { 5 }          // a blast's damage that sounds the alarm (Classic's heaviest)
fn SHAKE() { 2 }        // a blast's damage that flickers the strips near it
fn KILLS() { 3 }        // kills
fn KILL_SPAN() { 1000 } // within these ticks sound it too
fn ALARM() { 1000 }     // ticks it sounds (10 s)
fn REARM() { 2000 }     // ticks after it before another (20 s)
fn MAINTAIN() { 2000 }  // ticks between mendings (20 s)
fn EMERGENCY() { 1000 } // the emergency lamps' intensity in a blackout
fn NEAR() { 40 }        // px from a strip's centre a blast flickers it

// The level's lamp groups, their alarm intensity and the alarm's colour.
fn LAMP_GROUPS() { ["lamps", "cool", "sodium", "mine"] }
fn DIMMED() { 300 }
fn RED() { 0xff3a20 }
// The strips, and each one's own colour (`factory_live.py`'s STRIP_COLOURS).
fn STRIPS() { ["strip-1", "strip-2", "strip-3", "strip-4", "strip-5", "strip-6", "strip-7", "strip-8", "strip-9"] }
// Each strip's centre (its box's middle, whole pixels), for the blasts
// that flicker it: one handler here, not one a strip (a fight's 40 blasts
// a tick would cost ten runs each).
fn STRIP_AT() { [[792, 81], [1018, 81], [1133, 81], [998, 178], [1081, 178], [328, 287], [441, 287], [538, 287], [201, 343]] }
// Each strip's state at rest: the faulty tubes On (buzzing), the others
// Steady (`factory_live.py`'s LIVE_STRIPS).
fn STRIP_REST() { ["Steady", "On", "Steady", "On", "Steady", "Steady", "On", "Steady", "Steady"] }
fn STRIP_COLOURS() { [0xdce8ff, 0xdce8ff, 0xdce8ff, 0xffe6c0, 0xffe6c0, 0xffb050, 0xffb050, 0xffb050, 0xffb050] }

// `mem.alarm` and `mem.dark` the plant's state, `mem.rearm` the tick from
// which an alarm may sound again; `mem.k1`..`mem.k3` the last kills' times
// (ticks, -1 none).
fn start() {
  this.mem.alarm = false;
  this.mem.rearm = 0;
  this.mem.dark = false;
  this.mem.k1 = -1;
  this.mem.k2 = -1;
  this.mem.k3 = -1;
  this.after(QUIET(), "horn");
  this.every(MAINTAIN(), "maintain");
}

fn tick() {
  // The terminal.
  let terminal = this.copy("terminal");
  let alarm = this.mem.alarm || this.state(this.copy("crane")) == "Moving";
  let want = if alarm { "Alarm" } else { "Normal" };
  if this.state(terminal) != want { this.set_state(terminal, want); }
  // The generator.
  let wrecked = this.state(this.copy("generator")) == "Wrecked";
  if wrecked && !this.mem.dark { this.blackout(); }
  if !wrecked && this.mem.dark { this.power(); }
}

// --- the shift ------------------------------------------------------------
fn horn() {
  if this.mem.dark { return; }
  this.play(this.copy("line"), "horn.wav");
  this.after(HORN(), "run");
}

fn run() {
  if this.mem.dark { return; }
  this.set_state(this.copy("line"), "Lit 1");
  this.after(RUN(), "stand");
}

fn stand() {
  this.set_state(this.copy("line"), "Stopped");
  if !this.mem.dark { this.after(QUIET(), "horn"); }
}

// --- the alarm ------------------------------------------------------------
fn exploded(x, y, damage) {
  if damage < SHAKE() { return; }
  if damage >= BIG() { this.alarm(); }
  let at = STRIP_AT();
  for i in 0..at.len() {
    let dx = x - at[i][0];
    let dy = y - at[i][1];
    if dx * dx + dy * dy <= NEAR() * NEAR() { this.shake(i); }
  }
}

// A blast near strip `i`: lit, it is set Off, which its own script takes
// for a burst of flickers.
fn shake(i) {
  let c = this.copy(STRIPS()[i]);
  let s = this.state(c);
  if s == "On" || s == "Dim" || s == "Steady" { this.set_state(c, "Off"); }
}

fn killed(victim, killer) {
  let now = this.time() / 10;
  this.mem.k1 = this.mem.k2;
  this.mem.k2 = this.mem.k3;
  this.mem.k3 = now;
  if this.mem.k1 >= 0 && now - this.mem.k1 <= KILL_SPAN() { this.alarm(); }
}

fn alarm() {
  if this.mem.alarm || this.time() / 10 < this.mem.rearm { return; }
  this.mem.alarm = true;
  this.set_state(this.copy("siren"), "Alarm");
  this.lamps();
  this.after(ALARM(), "calm");
}

fn calm() {
  this.mem.alarm = false;
  this.mem.rearm = this.time() / 10 + REARM();
  this.set_state(this.copy("siren"), "Normal");
  this.lamps();
}

// The lamps as the plant's state has them: out in a blackout, red and
// dimmed in an alarm, else their own.
fn lamps() {
  for g in LAMP_GROUPS() { this.group_lamp(g); }
  let strips = STRIPS();
  let colours = STRIP_COLOURS();
  for i in 0..strips.len() { this.strip_lamp(strips[i], colours[i]); }
}

fn group_lamp(g) {
  if this.mem.dark { this.group_intensity(g, 0); return; }
  if !this.mem.alarm { this.group_reset(g); return; }
  this.group_colour(g, "#ff3a20");
  this.group_intensity(g, DIMMED());
}

fn strip_lamp(id, own) {
  let colour = own;
  if this.mem.alarm { colour = RED(); }
  this.set(this.copy(id), "colour", colour);
}

// --- the blackout ---------------------------------------------------------
fn blackout() {
  this.mem.dark = true;
  this.lamps();
  this.group_intensity("emergency", EMERGENCY());
  for s in STRIPS() {
    let c = this.copy(s);
    if this.state(c) != "Broken" { this.set_state(c, "Dark"); }
  }
  this.cancel("horn");
  this.cancel("run");
  this.cancel("stand");
  this.set_state(this.copy("line"), "Stopped");
  this.set_state(this.copy("crane"), "Off");
  this.set_state(this.copy("lift"), "Off");
  this.set_state(this.copy("cart"), "Off");
}

fn power() {
  this.mem.dark = false;
  this.lamps();
  this.group_reset("emergency");
  let strips = STRIPS();
  let rest = STRIP_REST();
  for i in 0..strips.len() {
    let c = this.copy(strips[i]);
    if this.state(c) == "Dark" { this.set_state(c, rest[i]); }
  }
  this.set_state(this.copy("crane"), "Idle");
  this.set_state(this.copy("lift"), "Idle");
  this.set_state(this.copy("cart"), "Stopped");
  this.after(QUIET(), "horn");
}

// --- maintenance ----------------------------------------------------------
fn maintain() {
  let strips = STRIPS();
  for i in 0..strips.len() {
    let c = this.copy(strips[i]);
    if this.state(c) == "Broken" { return this.mend(c, STRIP_REST()[i]); }
  }
}

fn mend(c, rest) {
  this.set(c, "hp", this.max(c, "hp"));
  let s = rest;
  if this.mem.dark { s = "Dark"; }
  this.set_state(c, s);
}
