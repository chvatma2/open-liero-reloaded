// The consoles in Alarm while the press moves.
fn tick() {
  if this.moving(this.copy("press"), "head_y") {
    this.alarm(this.copy("console-a"));
    this.alarm(this.copy("console-b"));
  }
}
fn alarm(c) { if this.state(c) != "Alarm" { this.set_state(c, "Alarm"); } }
