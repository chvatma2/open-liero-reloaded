# Oracle scenarios awaiting their traces

Scenarios written for issue #23 (Strafe, Previous/Next weapon) and issue
#48 (Weapon 1-5: `weapon-keys`) whose traces need Docker to record
(`docs/deferred-verification.md`). They live here, not in
`fixtures/oracle/`, because `olr-oracle`'s `tests/fixtures.rs` requires a
trace for every scenario there; `pending_scenarios_parse` checks these
parse. To record them:

1. `git mv fixtures/oracle-pending/*.ron fixtures/oracle/`;
2. `cargo xtask oracle-record strafe prev-next prev-next-no-combos weapon-keys`;
3. add the four names to `SPECIAL` in `crates/olr-oracle/tests/fixtures.rs`;
4. `olr-cli scenario strafe prev-next prev-next-no-combos weapon-keys` must pass bit for
   bit (`cargo xtask check`); then delete this directory and the
   `pending_scenarios_parse` test.

A trace whose client disallows strafing fails to record
(`OracleHooks::beforeFrame`: "strafing is not allowed on the client").
