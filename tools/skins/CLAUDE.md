# tools/skins

Scripts that refine OLR's own skins (`content/reloaded/`) by editing their
PNGs, after `olr-cli skin` made the starting point. Each runs with
`python3 -I` and only the standard library (its own PNG reader and
writer: 8-bit, non-interlaced); `olr-cli pack fmt` makes the result
canonical. The commands are in `content/reloaded/README.md`; they
reproduce the committed pack byte for byte.

- `firefly.py` (issue #13): Firefly from `olr-cli skin double
  olx-skin-default`: shading, rings, eyes, the gun's metal, the glowing
  tail lantern and its light, and OLX's default skin's origin.
