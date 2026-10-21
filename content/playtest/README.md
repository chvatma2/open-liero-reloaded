# Playtest packs

Hand-made packs for playtests, built with `olr-cli level …` from the
imported OLX packs in `content/packs` (which hold only what the OLX
import makes). They are not bundled with a release: copy a pack's folder
into the `packs/` folder of the game's data directory:

- Linux: `~/.local/share/open-liero-reloaded/packs/`
- Windows: `%APPDATA%\open-liero-reloaded\data\packs\`

or lay them over the content with `olr-cli render-match --packs content/playtest`.
`olr-cli`'s `tests/playtest.rs` checks them. Packs that ship with the
game go to `content/reloaded/` instead (the day Castle Strike did, as
Castle Strike Reloaded, issue #7).

- `olr-level-castlestrike-night/`: Castle Strike (night). OLX's Castle
  Strike with a look: a dark blue ambient; the sky glows (group `sky`, a
  pale blue tint of unlimited reach on a copy of the background layer,
  `background-2`), so moonlight comes from every open patch of sky through
  the glow light rather than from one direction; torches that flicker in
  the cellars and the gatehouse, lanterns in the towers, a cool light at
  the well, and glowing torch flames and candles (group `flames`). Its
  light objects went with plan 15c: its glowing pixels light it.
