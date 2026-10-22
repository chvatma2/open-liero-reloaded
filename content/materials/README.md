# Built-in surface materials

The game's own surface materials (issue #37; the format is
`docs/formats/material.md`): one `<id>.ron` each, read-only, shipped
beside the packs (`cargo xtask dist` copies this folder without this
README). The level editor and the material editor list them before the
user's own (`materials/` in the game's data directory).

Heat values are percent of stone's (100). A level holds copies of the
materials it uses, so changing a file here never changes a saved level.

Every file must read cleanly and be canonical (`olr-edit`'s
`tests/material.rs`, `the_built_in_set_loads_cleanly`).
