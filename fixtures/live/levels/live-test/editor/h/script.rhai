// Shut and open in turn every 250 ticks.
fn start() { this.every(250, "swing"); }
fn swing() { this.set("door_on", if this.get("door_on") == 0 { 1 } else { 0 }); }
