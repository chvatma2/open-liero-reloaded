# olr-vfs

Content I/O: the `ContentSource` / `ContentSink` traits, `MemFs` (tests, later
the web) and `DirFs` (native), plus case-insensitive lookup for OLX data.

- **Layer 1, uses `std`.** Everything else reaches files only through these traits.
- **`VPath` is the only path type.** It rejects `..`, absolute paths, empty
  components, backslashes and NUL, so content can never escape its root.
  OLX names with `\` go through `VPath::from_olx`.
- **Portable names** (`non_portable(name)`, plan 11 review C1): no `:`,
  `<>"|?*` or control characters, no trailing dot or space, no Windows
  device name (`CON`, `PRN`, `AUX`, `NUL`, `COM1`-`9`, `LPT1`-`9`, any case
  and extension). `VPath` itself allows them (OLX names are read as they
  are). `DirFs` builds a native path only from components that parse as
  exactly one `Component::Normal` (on Windows `C:` would replace the
  root), and a **write** only from portable names, on every platform: a
  pack written on Linux is the same pack on Windows.
- `list` returns sorted names with a trailing `/` for directories.
- **Bounded reads** (untrusted content): `read_max(path, limit)` refuses a
  longer file (`VfsError::TooLarge`; `DirFs` from its length, unread, and
  never reads past the limit if it grows), `read_prefix(path, n)` reads a
  header, `file_len(path)`. `DirFs` reads only regular files: a directory,
  FIFO or device is `VfsError::NotAFile` before it is opened (a FIFO would
  block), and `exists` is a metadata check, not a read.
- `find_ci` prefers exact matches, then the first case-insensitive match in sorted order.

Test: `cargo nextest run -p olr-vfs`.
