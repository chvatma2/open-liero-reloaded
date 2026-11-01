// Liero Factory Reloaded's level script (issue #97): the console is in
// Alarm while the crane moves, Normal otherwise, set only on a change.
fn tick() {
  let console = this.copy("console");
  if this.state(this.copy("crane")) == "Moving" {
    if this.state(console) != "Alarm" { this.set_state(console, "Alarm"); }
  } else if this.state(console) != "Normal" {
    this.set_state(console, "Normal");
  }
}
