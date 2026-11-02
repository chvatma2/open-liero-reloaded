// Liero Factory Reloaded's level script (issue #97): the terminal left of
// the crane (issue #100; the console under the deck before it) is in
// Alarm while the crane moves, Normal otherwise, set only on a change.
fn tick() {
  let terminal = this.copy("terminal");
  if this.state(this.copy("crane")) == "Moving" {
    if this.state(terminal) != "Alarm" { this.set_state(terminal, "Alarm"); }
  } else if this.state(terminal) != "Normal" {
    this.set_state(terminal, "Normal");
  }
}
