# tools/flatpak

The Flatpak of the Linux release, built by `cargo xtask flatpak`
(`xtask/src/flatpak.rs`) into
`target/release-zips/open-liero-reloaded-<version>-linux-x64.flatpak`.

- **Repackaged, not built from source.** The programs are `dist`'s
  (`dist::build`, the `dist` profile, built on this machine) staged as in
  the zip (`dist::stage`) under `target/flatpak/src/app/`, beside copies of
  the four files here (the metainfo's `@VERSION@` replaced by the
  workspace version). `flatpak-builder --user --install-deps-from=flathub
  --disable-rofiles-fuse` (state in `target/flatpak/state`) exports to
  `target/flatpak/repo`, then `flatpak build-bundle` (the runtime from
  Flathub) writes the bundle. Flathub would want a source build (vendored
  crates, the Rust SDK extension): not done.
- **Runtime** `org.freedesktop.Platform` 26.08 (glibc 2.44): a binary built
  on Ubuntu 26.04 needs glibc 2.43 (libm's `atan2f`, `acosf`, ...). An older
  runtime fails at start. `libasound.so.2` and `libudev.so.1`, the
  programs' other libraries, are in the runtime.
- **Layout**: `/app/lib/olr/` holds the release folder (programs, docs,
  `content/packs`); `/app/bin/olr` and `/app/bin/olr-server` are symbolic
  links to it, and `/proc/self/exe` resolves them, so the programs find
  the content beside them (`olr_content::default_content_dir`). The data
  directory (`directories`' XDG paths) is `~/.var/app/<id>/`.
- **Permissions**: Wayland with X11 as fallback, IPC, every device (the GPU
  and gamepads: gilrs reads `/dev/input`), PulseAudio (ALSA reaches it
  through the runtime's plugin), the network.
- **Application ID** `io.github.openlieroreloaded.OpenLieroReloaded`
  (`flatpak::APP_ID`): the files' names, and `olr-game`'s `window::APP_ID`,
  the window's Wayland app ID and X11 class, so desktops match the window
  to the desktop entry; a test keeps them equal. The icon is a placeholder.
- Install and run: `flatpak install --user <bundle>`, then
  `flatpak run io.github.openlieroreloaded.OpenLieroReloaded` (the menus) or
  `flatpak run --command=olr-server io.github.openlieroreloaded.OpenLieroReloaded --config <file>`.
- Prerequisites: `sudo apt install flatpak flatpak-builder` and `flatpak
  remote-add --user --if-not-exists flathub
  https://dl.flathub.org/repo/flathub.flatpakrepo`; without the tools the
  command fails saying so.
