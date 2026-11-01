// The Control desk's default script (issue #99): its lamp dots change now and
// then: every CHANGE_MIN() to CHANGE_MAX() ticks a new state, A, B or C,
// never the one it shows. Its constants (ticks are 10 ms; tuned in issue
// #100):
fn CHANGE_MIN() { 100 }   // ticks between changes, at least
fn CHANGE_MAX() { 400 }   // and at most

// A whole number from `lo` to `hi`, both included, from the scripts'
// generator (`this.rand`), so the same match seed changes alike.
fn between(lo, hi) { lo + this.rand(hi - lo + 1) }

fn start() {
  this.after(this.between(CHANGE_MIN(), CHANGE_MAX()), "change");
}

// One of the two states it does not show, then the next change.
fn change() {
  let others = [];
  for s in ["A", "B", "C"] {
    if s != this.state() { others.push(s); }
  }
  this.set_state(others[this.rand(2)]);
  this.start();
}
