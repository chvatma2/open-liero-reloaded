# rust-liero

A Rust reimplementation of [OpenLieroX](https://github.com/openlierox/openlierox),
built so physics, movement and shooting feel identical, and modernized for
today's displays. It loads classic OLX mods and levels by importing them into
an open, text-based pack format.

Status: early development. The current milestone imports OLX content.

## Quick start

```sh
cargo xtask check          # format, lint, layering, web build, OLX content, tests
cargo xtask fetch-olx-content
cargo run -p rl-cli -- validate target/content
```

## Credits

Based on OpenLieroX by Albert Zeyer, Karel Petránek and the OpenLieroX team,
which is based on Liero Xtreme by Jason Boettcher, which is inspired by Liero
by Joosa Riekkinen. Bundled OLX mods and levels belong to their authors.

## License

GPL-3.0-or-later. See `LICENSE`.
