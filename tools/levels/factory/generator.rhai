// The Generator's default script (the plant works, 2026-11-04): wrecked
// (its health gone, so Wrecked), it is repaired after REPAIR ticks, its
// health whole and Running again.
fn REPAIR() { 3000 }  // ticks wrecked (30 s)

fn entered(s) {
  if s == "Wrecked" { this.after(REPAIR(), "repair"); }
}

fn repair() {
  this.set("hp", this.max("hp"));
  this.set_state("Running");
}
