# Level composition fixtures (plan 14a)

`composed-level/` is Dirt Level composed of five elements and baked. Its
bake guards the image pipeline and the bake against change:
`olr-edit`'s `the_fixture_is_baked` holds its baked files equal to what its
composition bakes to, and `cargo xtask determinism` compares its bake's
digest (`olr-detcheck --bake`) on every platform.

Made with `olr-cli` from the repository root (`badge.png` from
`python3 badge.py badge.png`, a 16 × 16 disc with a half-transparent rim):

```
olr-cli pack new level composed-level --from content/packs/olx-level-dirt-level --out fixtures/edit/composed-level --name "Composed Level"
olr-cli level element add-image fixtures/edit/composed-level crates/olr-format/tests/data/tiny.jpg --at 40,30 --size 60x45 --pixel-size 3 --colours 4 --dither ordered --material rock --name Tiny
olr-cli level element add-image fixtures/edit/composed-level crates/olr-format/tests/data/gradient.jpg --at 300,200 --size 64x48 --pixel-size 2 --colours 16 --dither ordered --material dirt --name Gradient
olr-cli level element add-image fixtures/edit/composed-level badge.png --at -6,100 --size 48x48 --colours 8 --dither diffusion --material dirt --name Badge
olr-cli level element add-layer fixtures/edit/composed-level --material background --name Tunnels
olr-cli level element draw fixtures/edit/composed-level tunnels --circle 200,150,20 --colour 303030
olr-cli level element draw fixtures/edit/composed-level tunnels --rect 150,140,100,12 --colour 303030
olr-cli level bake fixtures/edit/composed-level
```

A change that is meant to change the bake re-runs `olr-cli level bake
fixtures/edit/composed-level` and commits the result with its reason.
