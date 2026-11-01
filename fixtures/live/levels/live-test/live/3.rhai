// The head goes to its other end every 2 s: 0.6 s moving (Moving: it
// hums), then a rest (Still).
fn start() { this.head_to(this.max("head_y")); }
fn arrived(name) { this.set_state("Still"); this.after(138, "back"); }
fn back() {
  let to = if this.get("head_y") == this.max("head_y") { this.min("head_y") } else { this.max("head_y") };
  this.head_to(to);
}
fn head_to(to) { this.set_state("Moving"); this.move_to("head_y", to, 10); }
