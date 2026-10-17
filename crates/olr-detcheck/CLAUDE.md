# olr-detcheck

Layer 6, std; a developer tool, not a game tool. `olr-detcheck --content <dir>
[--every N] <replay.olrp>...` replays recorded replays (keys only: bots'
own maths is not under test) and prints `<file name> <tick> <checksum hex>`
for tick 0, every `N` ticks (default 10) and the last tick. Exit status 1
when a replay's own recorded checksums do not match. `--bake <pack dir>`
(plan 14a) then prints `bake <sha256 hex>`: the digest of the composed
level's bake (`material.png`, `front.png`, `back.png` as `write_level_files`
encodes them, each prefixed with its 8-byte little-endian length), the
back tile from `--content`; without replays it prints only that line.

`cargo xtask determinism` records golden bot matches natively and compares
this tool's stream on native Linux with WASI (`wasm32-wasip1` under Node.js,
`tools/wasi-run.mjs`), Windows (`--windows`) and arm64 Linux under QEMU
(`--arm64`): rollback needs identical simulation everywhere (spec §9.5).
It must build for `wasm32-wasip1` and `aarch64-unknown-linux-gnu`.
