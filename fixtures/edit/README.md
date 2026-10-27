# Level composition fixtures (plan 14a)

`composed-level/` is Dirt Level composed of six elements and baked: a
raster base, three images (two masked), a drawn layer and a moved, mixed
layer with back art (plan 14d). Its
bake guards the image pipeline and the bake against change:
`olr-edit`'s `the_fixture_is_baked` holds its baked files equal to what its
composition bakes to, and `cargo xtask determinism` compares its bake's
digest (`olr-detcheck --bake`) on every platform. Issue #74 converted it
(`olr-cli level convert fixtures/edit/composed-level`, the bake unchanged):
its layers are the ground asset's, `editor/asset.ron`, placed alone by
`editor/placed.ron`, as the commands below now write them.

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
olr-cli level element add-layer fixtures/edit/composed-level --material background --name Wall
olr-cli level element draw fixtures/edit/composed-level wall --rect 380,40,60,40 --colour 506070
olr-cli level element add-layer fixtures/edit/composed-level --material dirt --name Crate
olr-cli level element draw fixtures/edit/composed-level crate --rect 390,50,30,30 --colour 8a5a2a
olr-cli level element draw fixtures/edit/composed-level crate --rect 400,60,8,8 --colour 909090 --material rock
olr-cli level element merge-down fixtures/edit/composed-level crate
olr-cli level element set fixtures/edit/composed-level wall --at 4,0
olr-cli level element mask fixtures/edit/composed-level badge --circle 18,124,6
olr-cli level element mask fixtures/edit/composed-level gradient --colour-at 310,210 --tolerance 20
olr-cli level bake fixtures/edit/composed-level
```

A change that is meant to change the bake re-runs `olr-cli level bake
fixtures/edit/composed-level` and commits the result with its reason.
