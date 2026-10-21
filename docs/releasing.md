# Releasing and updates

How a build reaches players and how the game updates itself (issue #4).
The code: `crates/olr-update` (formats, checks, the folder swap),
`crates/olr-game/src/update.rs` (the check, the install, the restart),
`xtask/src/dist.rs` and `xtask/src/publish.rs`.

## What the game does

A build made by `cargo xtask dist` knows its release repository
(`OLR_RELEASE_REPO`, `chvatma2/open-liero-reloaded`, baked in at build
time). At start it asks GitHub for the releases in the background; without
a network, on a timeout or an error it shows nothing and logs why. A local
build (`0.2.0-dev (local build)`) never checks and shows no update.

- **Channels.** Stable is final releases only; Pre-release also the
  `-dev.` builds. Options → Online → **Include pre-releases** picks it; until
  changed it follows the installed build (a `-dev.` build is on
  pre-releases). The Flatpak's channel is its branch (`stable` or `beta`).
- **The offer** is the newest release in the channel newer than the build
  (semver precedence; dev builds by their commit count). Never a downgrade.
  The main menu shows **Update available: <version>**; its screen shows the
  release's title and notes and an **Update** button. Nothing is forced.
- **Windows (the zip).** **Update** downloads the zip and its signature
  `<zip>.sig`, checks the signature with the key baked into the game
  (`tools/release/update-signing.pub`), that the signed version is the one
  offered and newer than the build, and the zip's size and SHA-256. Then
  it unpacks into `.olr-update/new/` beside `olr.exe` and swaps the folder
  in place: each file the release has replaces the old one (the old one
  moved to `.olr-update/old/`, `olr.exe` first: Windows lets a running
  program be renamed), files of the old release (`install-manifest.txt`)
  that the new one lacks are removed, and anything else, the player's, is
  kept. A journal makes it safe to interrupt: the next start finishes the
  swap. **Restart now** starts the new `olr.exe`. A folder the game
  cannot write to (Program Files) is said so, with the release page.
- **Linux (the Flatpak).** **Update** asks the Flatpak portal
  (`org.freedesktop.portal.Flatpak`, `CreateUpdateMonitor`, `Update`) to
  update the installed branch; no permission is added to the sandbox.
  **Restart now** starts the new build through the portal's `Spawn`. When
  the portal refuses (a release asking for more permissions needs the
  player's consent), the screen says "Update from your software centre or
  run `flatpak update …`" with a copy button. Switching channel: Options
  → Online shows the command that installs the other branch and
  uninstalls this one, with a copy button.

## Publishing a release

1. Final release only: set the workspace version (`Cargo.toml`) to it,
   commit, tag `v<version>` and push the tag.
2. `cargo xtask dist` (or `cargo xtask dist --final`).
3. Write the notes, Markdown (they show in the game as text).
4. `cargo xtask publish --notes notes.md`.

`publish` takes the newest Windows zip in `target/release-zips/` (or
`--version`) and:

0. refuses a `.dirty` build; a dev build whose base version is not above
   the newest `v<x.y.z>` tag (it would sort below that release and never
   be offered: **after each final release, bump the workspace version**,
   e.g. 0.2.0 → 0.2.1); a signing key that is not the committed
   `tools/release/update-signing.pub`'s; a GPG home without the secret
   key of the committed `tools/flatpak/olr-flatpak.gpg`; a Flatpak in
   `target/flatpak/repo` that is not this version; and, so the release
   can be made once the repository is live, `gh` not logged in, a release
   `v<version>` that exists already, a final release's tag not pushed or
   a dev build's commit not on GitHub;
1. signs the zip's manifest: `<zip>.sig` beside the zip;
2. commits the Flatpak into the local signed repository, on `beta` for a
   pre-release and on `stable` and `beta` for a final release, and warns
   when its permissions (`finish-args`) differ from the branch's previous
   build (the portal will not install it: players update from their
   software centre); prunes it to `--keep` (3) builds a branch and updates
   its summary; builds the release's bundle from it
   (`open-liero-reloaded-<version>-linux-x64.flatpak`, replacing `dist`'s:
   installing it subscribes to the repository, so it updates); pushes the
   repository, `.nojekyll`, `index.html`, `open-liero-reloaded.flatpakrepo`,
   `stable.flatpakref`, `beta.flatpakref` and the key as one fresh commit,
   force-pushed, to the Pages repository; and waits (up to 30 min) until
   `https://openlieroreloaded.github.io/flatpak/summary` is the new one;
3. creates the GitHub release `v<version>` (`gh release create`, a
   pre-release on the dev build's commit for `-dev.`) with the zip, its
   signature and the bundle, the notes from `--notes`.

A step that fails stops it with `FAIL publish: <why>`; running it again
is safe (the signature is rewritten; a branch gets one more commit).
Options: `--signing-key`, `--gnupg-home`, `--flatpak-repo`,
`--pages-remote`, `--pages-url`, `--keep`, and `--release-dir <dir>`,
which copies the release's files there instead of calling `gh` (a dry run
of the last step). It needs `flatpak`, `ostree`, `git`, `gpg`, `curl` and
`gh` (logged in). Tested against temporary directories only (`cargo test
-p xtask publish`), unsigned: the first real run is the maintainer's.

## The maintainer's setup

- **Update signing key** (ed25519): secret at
  `~/.config/olr-release/update-signing.pem` (PKCS#8 PEM, `openssl genpkey
  -algorithm ed25519`), public half committed as
  `tools/release/update-signing.pub` (hex) and baked into every build.
- **Flatpak repository key** (GPG, RSA 4096,
  `299C4EBC1F402E2FBBA74FBB0D79D2E80E5AA5AB`): keyring
  `~/.config/olr-release/gnupg` (`GNUPGHOME`), public key committed as
  `tools/flatpak/olr-flatpak.gpg` (armored), in every bundle,
  `.flatpakref` and `.flatpakrepo`. No passphrases.
- **The local Flatpak repository**: `~/.local/share/olr-release/flatpak-repo`
  (persistent, outside `target/`: it holds the branches' history that
  updates are computed from).
- **The Pages repository**: the GitHub organisation `openlieroreloaded`,
  repository `flatpak`, GitHub Pages serving its `main` branch's root at
  `https://openlieroreloaded.github.io/flatpak/` (a fixed URL in every
  installed bundle, `olr_update::flatpak::REPO_URL`). `publish` pushes to
  `git@github.com:openlieroreloaded/flatpak.git` over SSH.

**Back up** the local Flatpak repository and both keys, off this machine.

- Losing the **GPG key** means a new key, which no installed Flatpak
  trusts: every Linux player must reinstall.
- Losing the **ed25519 key** means Windows players cannot update in the
  game any more: a release whose updater accepts a new key must be
  installed by hand (download the zip).
- Losing the **Flatpak repository** is recoverable: a new one starts
  again from the next build (players update to it), but old builds are
  gone.

## Release notes for the first published release

Players who installed an older bundle (from `dist`, branch `master`, no
repository) get no updates. The notes should say: install the new bundle,
or

```sh
flatpak install --user https://openlieroreloaded.github.io/flatpak/stable.flatpakref
flatpak uninstall --user io.github.openlieroreloaded.OpenLieroReloaded//master
```

(`beta.flatpakref` for pre-releases). Settings stay: they are the app's,
not the branch's.
