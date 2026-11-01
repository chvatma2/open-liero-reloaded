// The Lit sign's default script (issue #99): the "13-37 -LD-" board stays
// Normal a long while, then now and then one of its faults shows a moment
// (the dash dark, the "-LD-" line dark, or half the 7 dark, equal odds)
// before it is Normal again. Its constants (ticks are 10 ms; tuned in
// issue #100):
fn NORMAL_MIN() { 300 }   // ticks Normal between faults, at least
fn NORMAL_MAX() { 900 }   // and at most
fn FAULT_MIN() { 20 }     // ticks a fault shows, at least
fn FAULT_MAX() { 80 }     // and at most

// A whole number from `lo` to `hi`, both included, from the scripts'
// generator (`this.rand`), so the same match seed shows the same faults.
fn between(lo, hi) { lo + this.rand(hi - lo + 1) }

fn start() {
  this.after(this.between(NORMAL_MIN(), NORMAL_MAX()), "fault");
}

fn fault() {
  let faults = ["No dash", "No LD", "Dying 7"];
  this.set_state(faults[this.rand(3)]);
  this.after(this.between(FAULT_MIN(), FAULT_MAX()), "mend");
}

fn mend() {
  this.set_state("Normal");
  this.start();
}
