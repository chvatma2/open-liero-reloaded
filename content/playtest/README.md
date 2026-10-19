# Playtest packs

Hand-made packs for playtests, built with `olr-cli level …` from the
imported OLX packs in `content/packs` (which hold only what the OLX
import makes). They are not bundled with a release: copy a pack's folder
into the `packs/` folder of the game's data directory:

- Linux: `~/.local/share/open-liero-reloaded/packs/`
- Windows: `%APPDATA%\open-liero-reloaded\data\packs\`

or lay them over the content with `olr-cli render-match --packs content/playtest`.
`olr-cli`'s `tests/playtest.rs` checks them.

- `olr-level-castlestrike-night/`: Castle Strike (night). OLX's Castle
  Strike with a look: a dark blue ambient, a pale moon high above the
  level (unlimited reach, so towers and walls throw long shadows), torches
  that flicker in the cellars and the gatehouse, lanterns in the towers,
  a cool light at the well, and glowing torch flames and candles (glow
  group `flames`).
