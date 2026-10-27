// The console's cycle by a timer of its own (its default is off).
fn start() { this.after(300, "next"); }
fn next() {
  this.set_state(if this.state() == "Alarm" { "Normal" } else { "Alarm" });
  this.after(300, "next");
}
