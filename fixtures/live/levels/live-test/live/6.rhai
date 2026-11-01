// Left and right at 20 px/s, resting a second at each end or where
// something stops it.
fn start() { this.move_to("lift_x", this.max("lift_x"), 20); }
fn arrived(name) { this.after(100, "turn"); }
fn blocked(name) { this.after(100, "turn"); }
fn turn() {
  let to = if this.get("lift_x") > 0 { this.min("lift_x") } else { this.max("lift_x") };
  this.move_to("lift_x", to, 20);
}
