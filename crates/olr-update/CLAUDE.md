# olr-update

Updating the game (issue #4): which release a build is offered and how a
download is trusted and installed. No networking and no UI: `olr-game`
fetches, downloads, talks to the Flatpak portal and fills `olr-ui`'s
views; `cargo xtask publish` signs and publishes with the same formats.

- **Layer 5, uses `std`.** Depends on serde and serde_json (GitHub's
  release list), `aws-lc-rs` 1.18.1 (rustls' provider, already built:
  ed25519 and SHA-256), base64 (PKCS#8 PEM), `zip` 8.6.0 with flate2's
  pure-Rust backend (the Windows zip). Used by `olr-game` and `xtask`.
- **Versions** (`version.rs`): `Version::parse` is tolerant semver (a
  leading `v`, `+build` dropped; `0.2.0-dev (local build)` is none).
  `precedence` is semver's, except a `dev` build is ordered by `dev.<count>`
  alone: its commit (may be all digits with a leading zero) and `.dirty`
  never order. `is_dirty`, `base`, `is_newer_than`.
- **Releases** (`release.rs`): `parse_releases` reads GitHub's list
  (`/repos/<owner>/<repo>/releases`; drafts, tags that are no version and
  non-`https` asset URLs dropped; at most `MAX_LIST_BYTES`). `Channel::{Stable,
  PreRelease}`: `of_build` (a `-…` build is on pre-releases), `of_setting`
  (the setting "Include pre-releases", `None` = the build's), `includes`
  (Stable: not marked pre-release and no `-…` version). `newest(releases,
  current, channel)`: the newest strictly newer one, never a downgrade or a
  `.dirty` build. Asset names as `dist` and `publish` write them:
  `windows_zip_name`, `flatpak_bundle_name`, `folder_name`,
  `signature_name` (`<file>.sig`). `lib.rs`' `channel(setting, build,
  flatpak)`: under Flatpak the branch's (`stable`/`beta`), else the
  setting's.
- **Signed manifest** (`manifest.rs`): `<zip>.sig` is `MAGIC`
  (`olr-update-manifest 1`), `version`, `file`, `size`, `sha256` lines and
  a `signature` line (ed25519 over every byte before it). `verify(text,
  key)` refuses anything not signed by the key, then reads strictly;
  `Manifest::check(offered, current, file, size, sha256)` requires the
  signed version to be the one offered and newer than the build, and the
  file, size and digest to match (an older signed zip cannot pass for a
  newer release). `release_key()` is `tools/release/update-signing.pub`
  (hex, baked in with `include_str!`); `SigningKey::from_pem` (PKCS#8 v1
  or v2, `openssl genpkey -algorithm ed25519`), `sign`, `public`.
  `Sha256` hashes a download as it comes.
- **Portable folder** (`install.rs`, Windows): `check_writable` (the
  work directory `.olr-update/` and a file beside the program),
  `stage_zip(zip, game, top, exe)` (into `.olr-update/new/`, the zip's
  one top folder stripped, `safe_relative` paths only, no links, the work
  directory never, within `MAX_FILES`/`MAX_UNPACKED_BYTES`, the program
  required; writes the new `install-manifest.txt`, `MANIFEST_FILE`),
  `Plan::new` (install every staged file, the program first; remove the
  old manifest's files the new one lacks, compared ignoring case; files
  in no manifest, the player's, stay), `swap` (the journal `swap.txt`
  written atomically, then each step a rename, retried for about a
  second: the current file aside to a fresh `old/` (`old-<n>/` when an
  earlier one cannot be removed, its program still running; named in the
  journal), the staged one in, the old one put back when it cannot move
  in; emptied folders of removed files go; the journal removed last).
  `claim`: the work directory's lock (`try_lock`), held by an install,
  so another copy of the game's `recover` returns `Busy` and leaves it.
  `recover(game, exe)` at start: a journal is finished (idempotent, roll forward; `restart` when the program itself
  was swapped only now, so the old build is running), else the work
  directory removed (`old/`'s program, which ran, goes then). The only
  moment without a program is between its own two renames.
- **Flatpak** (`flatpak.rs`): `APP_ID`, `REPO_URL`
  (`https://openlieroreloaded.github.io/flatpak/`, in every bundle for
  good), branches `STABLE` and `BETA` (`branches_for`: a final release
  goes to both), `gpg_key_base64` (an armored key's `GPGKey=` value),
  `flatpakref`/`flatpakrepo` (the files `publish` puts in the
  repository), `parse_info` (`/.flatpak-info`: app ID, branch, per-user
  or system-wide), `FlatpakInfo::{channel, update_command,
  switch_command}` (install the other branch from its `.flatpakref` and
  uninstall this one), `PortalProgress::of` (the portal's `Progress`
  signal) and `needs_permissions` (the portal refuses an update asking
  for more permissions).
- **Notes** (`notes.rs`): `plain_text(markdown)`: headings, emphasis,
  links (`text (url)`), code marks, tags and comments become text, list
  items `•`, at most `MAX_NOTES_CHARS`.

Never panics on what GitHub, a zip, a journal or a file says. Tests:
`tests/releases.rs` (the list, every channel's offer, no downgrade),
`tests/install.rs` (the swap; an interruption before every step, then
`recover`; bad zips; a folder without a manifest; a read-only folder),
unit tests per module.
