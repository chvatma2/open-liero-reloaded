# rl-scenario

Plays oracle scenarios (`fixtures/oracle/*.ron`) in `rl-sim` and records
traces in `rl-oracle`'s format.

- **Layer 4, uses `std`.**
- The scenario's world comes from the imported packs in a content directory
  (`target/content`, written by `rl-cli import`) plus OLX's settings from the
  trace header.
- Weapon slots, lives and kills are copied from OLX's frame −1 until M4
  simulates weapon selection.
- `tests/oracle.rs` is the acceptance test: every scenario must match bit for
  bit in the fields `CHECKED` lists, except the scenarios it lists as expected
  to diverge (which must still diverge). Grow `CHECKED` as M4 lands.
- For debugging a divergence: `rl-cli scenario <name> --dump <frame>` (`--all`
  compares every field, not only `CHECKED`).

Test: `cargo nextest run -p rl-scenario` (with `RL_CONTENT_DIR`; `cargo xtask check` sets it).
