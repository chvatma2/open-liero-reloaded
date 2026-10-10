# rl-vfs

Content I/O: the `ContentSource` / `ContentSink` traits, `MemFs` (tests, later
the web) and `DirFs` (native), plus case-insensitive lookup for OLX data.

- **Layer 1, uses `std`.** Everything else reaches files only through these traits.
- **`VPath` is the only path type.** It rejects `..`, absolute paths, empty
  components, backslashes and NUL, so content can never escape its root.
  OLX names with `\` go through `VPath::from_olx`.
- `list` returns sorted names with a trailing `/` for directories.
- `find_ci` prefers exact matches, then the first case-insensitive match in sorted order.

Test: `cargo nextest run -p rl-vfs`.
