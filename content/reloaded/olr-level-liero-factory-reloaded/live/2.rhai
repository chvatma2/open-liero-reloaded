// The Computer screen's default script (issue #99): a security camera
// showing each part of the factory in turn, Truck, Crane, Offices and
// Garden, then again, each for SHOW() ticks. No randomness: a camera cycle.
// The plant works (2026-11-04): a kill cuts to the camera nearest the
// victim for CUT() ticks, then the cycle goes on.
// Its constants (ticks are 10 ms; tuned in issue #100):
fn SHOW() { 250 }   // ticks each picture shows (2.5 s)
fn CUT() { 400 }    // ticks a kill's camera shows (4 s)

// Each camera's state and the world point it looks at (its picture's
// part of the factory: `factory_live.py`'s SCREEN_PARTS, the truck's the
// dock).
fn CAMERAS() { ["Truck", "Crane", "Offices", "Garden"] }
fn LOOKS() { [[100, 420], [790, 296], [525, 228], [505, 154]] }

fn start() {
  this.mem.cut = false;
  this.every(SHOW(), "next");
}

// The next camera after the one showing (none while a cut shows).
fn next() {
  if this.mem.cut { return; }
  let s = this.state();
  let n = if s == "Truck" { "Crane" } else if s == "Crane" { "Offices" } else if s == "Offices" { "Garden" } else { "Truck" };
  this.set_state(n);
}

fn killed(victim, killer) {
  if victim < 0 { return; }
  let x = this.worm_x(victim);
  let y = this.worm_y(victim);
  let looks = LOOKS();
  let best = 0;
  let near = -1;
  for i in 0..looks.len() {
    let dx = x - looks[i][0];
    let dy = y - looks[i][1];
    let d = dx * dx + dy * dy;
    if near < 0 || d < near { near = d; best = i; }
  }
  this.mem.cut = true;
  let s = CAMERAS()[best];
  if this.state() != s { this.set_state(s); }
  this.after(CUT(), "resume");
}

fn resume() { this.mem.cut = false; }
