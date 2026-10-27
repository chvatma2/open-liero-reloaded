// Stutters: now and then Dim or Off for a few ticks.
fn start() { this.after(1 + this.rand(50), "stutter"); }
fn stutter() {
  if this.state() == "On" {
    this.set_state(if this.rand(3) == 0 { "Off" } else { "Dim" });
    this.after(3 + this.rand(8), "stutter");
  } else {
    this.set_state("On");
    this.after(20 + this.rand(120), "stutter");
  }
}
