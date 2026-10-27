// Normal and Alarm in turn every 300 ticks.
fn start() { this.every(300, "next"); }
fn next() { this.set_state(if this.state() == "Normal" { "Alarm" } else { "Normal" }); }
