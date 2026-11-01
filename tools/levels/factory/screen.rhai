// The Computer screen's default script (issue #99): a security camera
// showing each part of the factory in turn, Truck, Crane, Offices and
// Garden, then again, each for SHOW() ticks. No randomness: a camera cycle.
// Its constant (ticks are 10 ms; tuned in issue #100):
fn SHOW() { 250 }   // ticks each picture shows (2.5 s)

fn start() {
  this.every(SHOW(), "next");
}

// The next camera after the one showing.
fn next() {
  let s = this.state();
  let n = if s == "Truck" { "Crane" } else if s == "Crane" { "Offices" } else if s == "Offices" { "Garden" } else { "Truck" };
  this.set_state(n);
}
