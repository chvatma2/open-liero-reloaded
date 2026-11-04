// The Siren's default script (the plant works, 2026-11-04): while in Alarm
// its two rings' beams turn, as the Crane's warning beacon does (two beams
// 180 degrees apart a ring, only their spread, reach and light changing as
// it turns); Normal puts them out (its state values).
fn TURN() { 120 }    // ticks a turn (1.2 s: faster than the crane's)
fn PEAK() { 1000 }   // the beams' intensity side-on
fn REACH() { 150 }   // their reach side-on

// The crane's tables for a half-angle of 10 degrees, every 3 ticks of a
// quarter turn of 300 ticks (`crane.rhai`), read here at TURN()'s pace:
// the apparent spread, and the reach and light in hundredths of theirs.
fn SPREAD() { [10, 10, 10, 10, 10, 11, 11, 11, 11, 12, 12, 13, 14, 15, 16, 17, 19, 21, 24, 28, 34, 44, 68, 89, 89, 89] }
fn FAR() { [100, 100, 99, 98, 97, 95, 93, 91, 88, 84, 81, 77, 73, 68, 64, 59, 54, 48, 43, 37, 31, 25, 19, 13, 6, 0] }
fn LIGHT() { [100, 100, 100, 100, 100, 100, 100, 100, 100, 100, 100, 100, 100, 100, 100, 100, 100, 100, 100, 88, 56, 22, 1, 0, 0, 0] }

fn start() { this.mem.turn = 0; }

// Table `t` at a quarter turn's tick `q` (0 to TURN() / 4), in steps of
// the table's 25.
fn at(t, q) {
  let p = q * 75 / (TURN() / 4);
  let i = p / 3;
  let f = p % 3;
  if i >= 25 { t[25] } else if f == 0 { t[i] } else { t[i] + (t[i + 1] - t[i]) * f / 3 }
}

fn tick() {
  if this.state() != "Alarm" { return; }
  let h = this.mem.turn % (TURN() / 2);
  this.mem.turn = (this.mem.turn + 1) % TURN();
  let q = if h > TURN() / 4 { TURN() / 2 - h } else { h };
  let spread = this.at(SPREAD(), q);
  this.set("spread_a", spread);
  this.set("spread_b", spread);
  this.set("spread_c", spread);
  this.set("spread_d", spread);
  let reach = REACH() * this.at(FAR(), q) / 100;
  this.set("reach", if reach < 1 { 1 } else { reach });
  this.set("light", PEAK() * this.at(LIGHT(), q) / 100);
}
