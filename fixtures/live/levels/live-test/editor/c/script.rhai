// The head goes to its other end every 2 s: 0.6 s moving, then a rest.
fn start() { this.move_to("head_y", this.max("head_y"), 10); }
fn arrived(name) { this.after(138, "back"); }
fn back() {
  let to = if this.get("head_y") == this.max("head_y") { this.min("head_y") } else { this.max("head_y") };
  this.move_to("head_y", to, 10);
}
