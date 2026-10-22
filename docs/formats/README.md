# Pack formats: a guide

OLR reads its content from **packs**: directories of readable text files
(RON) and PNG images. This guide explains the model and shows one complete
example of each kind. The reference pages beside it list every field with
its type, default and meaning. They are generated from the code by
`cargo xtask format-docs` and checked by `cargo xtask check`:

- [`mod.md`](mod.md): weapons, projectiles and physics;
- [`level.md`](level.md): levels;
- [`skin.md`](skin.md): worm skins;
- [`theme.md`](theme.md): level themes (holes, green dirt, tiles);
- [`core.md`](core.md): the core pack (OLX's shared game assets);
- [`pack.md`](pack.md): `pack.ron`, every pack's manifest.
- [`asset.md`](asset.md): the level editor's assets, ready-made pieces
  of a level (not packs: a folder each).
- [`material.md`](material.md): surface materials, what a level's
  pixels are made of for the look (not packs: a file each).

An agent or a person can learn the formats from these pages alone.

## The model

### Packs and kinds

A pack is a directory holding `pack.ron` and the files of its kind:

| Kind | Files |
|---|---|
| mod | `mod.ron`, `weapons/<id>.ron`, `projectiles/<id>.ron`, `restrictions.ron`, assets in `gfx/` and `sfx/` |
| level | `level.ron`, `material.png`, `front.png`, `back.png`, `thumb.png` |
| skin | `skin.ron`, `skin.png`, `tint.png`, optional `glow.png` |
| theme | `theme.ron`, `holes/<i>.png` with `holes/<i>.mask.png`, `green_ball.png` with `green_ball.mask.png`, `stones/<i>.png`, `misc/<i>.png`, `front_tile.png`, `back_tile.png` |
| core | `core.ron`, `gfx/<id>.png`, `sounds/<file>` |

`pack.ron` names the pack's kind, its id (a lowercase slug such as
`twin-caves`, unique among installed packs) and the format version, 5.
A file the format does not read (an editor's backup, `.DS_Store`, a
README) is kept as it is, and `olr-cli pack check` warns about it.

### Canonical form

Every text file has one canonical form: RON with 4-space indents, no
struct names, fields in their declared order, fields at their default
left out, LF line ends and a final newline. The tools write only this
form, so a saved file differs from the old one only where a value
changed. `olr-cli pack fmt` rewrites a pack's files in canonical form, and
`olr-cli pack fmt --check` lists the files that are not.

Comments are not kept: the writer makes the text from the values, so a
comment would vanish at the next save. Write notes in the `notes:` field
instead, which every file but `pack.ron` and `restrictions.ron` has; the
game ignores them.

`pack.ron` is the exception: it is plain RON, every field written and an
optional value spelled `Some(…)` or `None` (see the manifest example).

### Units

Units are named, never rescaled. A field holds OLX's value in OLX's unit,
and its meaning in the reference says which: pixels (px) and px/s for
positions and speeds, px/s² for gravity, seconds, degrees, health points.
Names carry the unit where it helps (`between_shots_s`, `every_s`,
`rotation_step_deg`). A few units are OLX's own, such as a weapon's
`recharge` in full magazines per second at the game's LoadingTime 100 %.

### Events and effects

A projectile reacts to events, each with a list of effects:

- `hit_terrain`: it touches rock or dirt;
- `hit_worm`: it touches a worm;
- `timer: (after_s: …, variation_s: …, effects: [...])`: its `after_s`
  seconds of life (± `variation_s`) have passed.

An empty list is OLX's `Nothing`, which still shapes the physics:
`hit_terrain: []` moves the projectile half a step into the terrain, and
`hit_worm: []` turns its worm collision off. The effect kind on
`hit_terrain` also chooses how the projectile collides (`Bounce`
bounces, `GoThrough` passes); the reference's `Projectile` fields say
how. An effect such as `Explode(damage: 4)` acts
on the projectile and around it; `Spawn("<name>")` makes the named entry
of the projectile's `spawns` (a child spawn: how many children, how fast,
in which directions, and which projectile). The runtime follows OLX
(LX56): an event holds at most one effect besides `Spawn`, and a
projectile has at most one named spawn.

### Shared and inline projectiles

A spawn's projectile is either defined in place (`projectile: (...)`) or
shared: `shared: "<id>"` names `projectiles/<id>.ron`, for a projectile
that several spawns make. Every projectile has a key, by which the
compiled mod's definitions name their children (the simulation never
reads the key strings): a shared projectile's key is its file id; an inline projectile's key is its `id` when set, and
otherwise comes from its place: the weapon id for a shot, then
`<parent>.spawn` for an event spawn and `<parent>.trail` for a trail. Two
projectiles with the same key, and cycles between shared projectiles, are
errors.

### Objects

A level's `objects` are things placed in it, addressed by their index in
the list. Today there is one kind, `Spawn(at: (x, y))`, a spawn point in
world pixels; the runtime does not read spawns yet.

### Sprites and art scale

Images can be drawn at a finer resolution than the world. A pack's
`art_scale` is how many art pixels make one world pixel: a level of
`size: (320, 200)` at `art_scale: 2` has `front.png` and `back.png` of
640 × 400. Skins and the core sprites have an art scale too. A
projectile's image strip holds square frames side by side: its frame
count is the width divided by the height. Images have real alpha;
nothing is colour-keyed when it loads.

### Materials and the palette

`material.png` is an 8-bit palette image of the level's size, one index
per world pixel: 0 rock (grey), 1 background (black), 2 dirt (brown).
Worms and projectiles collide with rock and dirt; weapons carve dirt.

### The front/back invariant

`front.png` is what the player sees; `back.png` is what shows where dirt
has been carved away. Wherever the material is background, `front`
equals `back`. The level operations keep it so: carving copies the back
to the front, and painting the back also paints the front there.

### Composed levels

A level can be composed of **elements**, stacked bottom to top in
`editor/composition.ron`, and **baked** into the files the game reads:
`material.png`, `front.png`, `back.png` and `thumb.png`. An element is:

- an **image**: a PNG or JPEG kept as imported in `editor/images/`,
  placed in a box (`at`, `size`, world pixels; it may reach outside the
  level) and pixelated (`pixel_size`, `colours`, `dither`,
  `alpha_threshold`);
- a **drawn** layer, `editor/layers/<id>.png`, art of the level's size
  whose alpha says which pixels it covers;
- a **raster** element, a whole level's own material, front and back
  (`editor/layers/<id>.material.png`, `.front.png`, `.back.png`). A level
  first composed (an OLX level, a `pack new` level) becomes one raster
  element, `base`, so its bake gives back its own files byte for byte.

Image and drawn elements have one material each: rock, dirt or
background. **Topmost wins:** a world pixel's material is the topmost
visible element covering it, and its art the topmost covering each art
pixel. **Background elements are the backdrop:** where dirt is carved in
play, the topmost background element beneath shows, else a raster's
back, else the theme's back tile. Hidden elements are left out.

`editor/` is the pack's authoring folder: it is never part of the
content hash and never in an archive, so players download only the
bake. The composition is the source of truth: `olr-cli pack check`
reports a baked file that differs from what the composition bakes to,
and `olr-cli level bake` rebakes it. The bake is integer arithmetic
only, the same bytes on every platform. A JPEG original is turned by its
EXIF orientation when baked; the file keeps its bytes.

### Meaning masks

Theme stamps and skins mark what each pixel means in a separate mask:

- a hole stamp, `holes/<i>.mask.png`: 0 untouched, 1 carve, 2 paint with
  the colour in `holes/<i>.png`;
- the green ball, `green_ball.mask.png`: 0 untouched, 1 green, 2 rim;
- a skin's `tint.png`: grey, 0 keeps the skin's colour, 255 takes the
  player's colour;
- a skin's optional `glow.png`: grey, 0 no glow to 255 full (in
  quarters): the pixel shows bright in the dark, still tinted.

## Examples

Each block below is checked: it parses as the file it names, it is
canonical, and the mod and the level make packs that `olr-cli pack check`
accepts. The reference pages show the same blocks.

### A mod

`mod.ron` lists the weapons; each is a file in `weapons/`. This bazooka
fires one missile, which explodes and makes twelve shrapnel
projectiles; the shrapnel is shared, so it lives in `projectiles/`.

<!-- file: mod/mod.ron -->
```ron
(
    name: "Rocket Mod",
    notes: "One weapon, to start from.",
    worm: (
        gravity: 100.0,
        jump_speed: -65.0,
    ),
    weapons: [
        "bazooka",
    ],
)
```

<!-- file: mod/weapons/bazooka.ron -->
```ron
(
    name: "Bazooka",
    recharge: 0.171,
    drain: 33.334,
    between_shots_s: 1.071,
    fire: Projectile(
        recoil: 15,
        sound: "sfx/bazooka.ogg",
        shot: (
            speed: 140,
            projectile: (
                gfx: Image(
                    file: "gfx/missile.png",
                    frames_follow_heading: true,
                    heading_frames: 18,
                ),
                gravity: 0,
                velocity_factor: 1.001,
                trail: Smoke,
                hit_terrain: [
                    Explode(
                        damage: 4,
                        shake: 2,
                        sound: "sfx/exp2.ogg",
                    ),
                    Spawn("spawn"),
                ],
                hit_worm: [
                    Explode(
                        damage: 27,
                    ),
                    Spawn("spawn"),
                ],
                spawns: {
                    "spawn": (
                        amount: 12,
                        speed: 105,
                        speed_var: 14.0,
                        spread: 360.0,
                        shared: "shrapnel",
                    ),
                },
            ),
        ),
    ),
)
```

<!-- file: mod/projectiles/shrapnel.ron -->
```ron
(
    gfx: Pixels([
        "#b0b0b0",
    ]),
    gravity: 70,
    hit_terrain: [
        Explode(
            damage: 2,
            shake: 1,
        ),
    ],
    hit_worm: [
        Injure(
            damage: 2,
        ),
    ],
)
```

`restrictions.ron` holds the weapon-restriction presets by name: each
weapon (by its `name`) is `Enabled`, `Bonus` (only from bonuses) or
`Banned`.

<!-- file: mod/restrictions.ron -->
```ron
{
    "Standard 100lt": {
        "Bazooka": Enabled,
    },
}
```

The mod editor keeps its own file in `editor/` (as a level keeps its
composition there): the mod this one was copied from, whose values it
shows beside the fields. The game never reads it.

<!-- file: mod/editor/mod.ron -->
```ron
(
    base: "olx-classic",
)
```

### A level

<!-- file: level/level.ron -->
```ron
(
    name: "Twin Caves",
    notes: "Two caves joined by a tunnel.",
    size: (320, 200),
    theme: "olx-theme-dirt",
    objects: [
        Spawn(
            at: (40, 150),
        ),
        Spawn(
            at: (280, 150),
            notes: "the right cave",
        ),
    ],
)
```

A composed level adds `editor/`: its raster base (the level as it was),
an imported sign and a tunnel drawn as background:

<!-- file: level/editor/composition.ron -->
```ron
(
    elements: [
        (
            id: "base",
            name: "Base",
            kind: Raster,
        ),
        (
            id: "sign",
            name: "Sign",
            kind: Image(
                material: Rock,
                source: "sign.png",
                at: (140, 60),
                size: (40, 20),
                pixel_size: 2,
                colours: 8,
                dither: Ordered,
            ),
        ),
        (
            id: "tunnel",
            name: "Tunnel",
            kind: Drawn(
                material: Background,
            ),
        ),
    ],
)
```

### An asset

Not a pack: the level editor's ready-made piece of a level, a folder
`<asset-id>/` holding `asset.ron`, its elements' files without `editor/`
(here `layers/torch.png` and `glow/torch.png`, each 14 × 16) and
`thumb.png`. Its elements are a composition's, placed from the asset's
top-left corner, and it carries the glow groups they name (and the
surface materials, `materials`, with their painted maps in `surface/`;
placing joins those a level has by id and adds the others):

<!-- file: asset/asset.ron -->
```ron
(
    name: "Torch",
    size: (14, 16),
    elements: [
        (
            id: "torch",
            name: "Torch",
            kind: Drawn(
                material: Background,
                size: (14, 16),
            ),
            glow: (
                strength: 100,
                group: "flames",
                mask: true,
            ),
        ),
    ],
    glow_groups: [
        (
            id: "flames",
            intensity: 900,
            reach: Pixels(100),
            animation: Flicker(
                speed: 9,
                depth: 35,
            ),
        ),
    ],
)
```

### A surface material

Not a pack either: one file `<id>.ron`, the game's built-in ones in
`content/materials/` and the user's in `materials/` of the game's data
directory. A level copies the materials it uses into its look
(`look.materials`), and its elements name them by id (`surface`,
`back_surface`); the bake writes `look/surface.png` and
`look/surface-back.png`, one value per world pixel, `v` the look's
`v`-th material and 0 none. The built-in glass glows amber when hot and
lights little around it:

<!-- file: material/glass.ron -->
```ron
(
    id: "glass",
    name: "Glass",
    notes: "Glows amber when hot and lights little around it.",
    heat: (
        capacity: 90,
        conductivity: 40,
        cooling: 70,
        look: (
            ramp: [
                (
                    temperature: 120,
                    colour: "#401800",
                    glow: 0,
                ),
                (
                    temperature: 400,
                    colour: "#d06010",
                    glow: 60,
                ),
                (
                    temperature: 1000,
                    colour: "#ffd890",
                    glow: 100,
                ),
            ],
            light: 50,
        ),
    ),
)
```

From the command line (`olr-cli`'s `CLAUDE.md`): `olr-cli material list`
prints the library, `olr-cli level material add <pack> stone wood`
copies materials into a level, `olr-cli level element set <pack> rock
--surface stone` gives an element one, and `olr-cli level element
surface <pack> rock --rule 'rock:808080/20=stone' --rule '*=none'`
paints its map by collision kind and art colour.

### A skin

OLX's layout: 7 aim columns by 3 walk rows of 20 × 18 frames at art
scale 2, so `skin.png` and `tint.png` are 140 × 54. Double resolution
(`art_scale: 4, frame_size: (40, 36)`) makes them 280 × 108.

<!-- file: skin/skin.ron -->
```ron
(
    name: "Explorer",
    art_scale: 2,
    frame_size: (20, 18),
    aim_frames: 7,
    walk_frames: 3,
)
```

A double-resolution skin whose `glow.png` lights a visor, and its light:

<!-- file: skin/skin.ron -->
```ron
(
    name: "Night Explorer",
    art_scale: 4,
    frame_size: (40, 36),
    aim_frames: 7,
    walk_frames: 3,
    light: (
        intensity: 150,
        reach: 24,
    ),
)
```

### A theme

<!-- file: theme/theme.ron -->
```ron
(
    name: "Dirt",
    holes: 5,
    stones: 8,
    misc: 8,
    green_ball: true,
)
```

### The core pack

An excerpt; the bundled core pack lists every sprite, sound and colour
OLX shares between mods.

<!-- file: core/core.ron -->
```ron
(
    notes: "An excerpt of the core pack.",
    sprites: {
        "crosshair": (
            frames: (2, 1),
            art_scale: 2,
        ),
    },
    sounds: [
        "throw.ogg",
    ],
    colours: {
        "RopeColour1": "#a05000",
    },
)
```

### A manifest

<!-- file: pack/pack.ron -->
```ron
(
    format_version: 5,
    id: "twin-caves",
    name: "Twin Caves",
    version: "0.1.0",
    kind: Level,
    authors: [
        "You",
    ],
    origin: None,
    license: Some("CC-BY-4.0"),
)
```

## The workflow

1. **Start a pack.** `olr-cli pack new mod my-mod` writes a one-weapon mod
   to `./my-mod`; `olr-cli pack new level my-level --size 640x480` a
   plain level of dirt in a rock border. Themes and core packs start
   from a copy: `olr-cli pack new theme my-theme --from
   content/packs/olx-theme-dirt`. Skins have commands of their own:
   `olr-cli skin new my-skin --name "My Skin" [--from <skin>] [--double]`
   (blank, or a copy of any installed skin by id), `olr-cli skin double
   <skin> --out <dir>` (a classic skin doubled by Scale2x, to refine),
   `olr-cli skin check <dir>…` and `olr-cli skin preview <dir> --colour
   RRGGBB --out f.png` (every frame, both ways, light and dark).
2. **Edit the text files.** Look fields up in the reference pages.
3. **Check.** `olr-cli pack check my-mod` parses, compiles and validates
   the pack. Errors name the file and, for text that does not parse, the
   line and column; for a valid file with a wrong value, the field path.
   It exits with status 1 on any error.
4. **Format.** `olr-cli pack fmt my-mod` rewrites the files in canonical
   form; `--check` only lists those that are not.
5. **Edit a level by operation.** In world pixels, shapes as
   `--rect X,Y,W,H` or `--circle X,Y,R`, colours as `RRGGBB`:
   - `olr-cli level carve <pack> --circle 100,100,20` carves to background;
   - `olr-cli level fill <pack> --rect 0,190,320,10 --material rock --colour 808080`
     lays rock or dirt and paints its front;
   - `olr-cli level paint <pack> --rect … --layer back --tile bricks.png`
     paints an art layer;
   - `olr-cli level stamp <pack> --at 10,10 --image sign.png --layer front`
     draws an image on an art layer;
   - `olr-cli level resize <pack> --size 400x200 --offset 40,0` grows or
     shrinks the level;
   - `olr-cli level object list|add|move|remove <pack> …` edits the objects.

   Each operation saves the whole pack at once (the old pack or the new
   one, never a mix) and refreshes `thumb.png`; an operation whose result
   would not check writes nothing.
6. **Compose a level.** The first `level element` command on a plain
   level makes it one raster element, `base`; each command then bakes
   and saves (`--content` names the theme packs, `content/packs` by
   default):
   - `olr-cli level element add-image <pack> photo.jpg --at 0,0 --size
     320x200 --pixel-size 2 --colours 16 --dither ordered --material dirt`
     imports a picture;
   - `olr-cli level element add-layer <pack> --material background --name
     Caves` adds a drawn layer, and `level element draw|erase <pack> caves
     --circle 100,100,20 [--colour 202020]` paints it;
   - `olr-cli level element set <pack> <id> --visible false` (and `--name`,
     `--locked`, `--material`, `--at`, `--size`, `--pixel-size`,
     `--colours N|none`, `--dither`, `--threshold`), `level element
     raise|lower|remove <pack> <id>`;
   - `olr-cli level element list <pack>` prints the stack, bottom first;
   - `olr-cli level bake <pack>` rebakes after a hand edit.

   On a composed level `level carve|fill|paint|stamp` are refused (edit
   its elements instead); `level resize` resizes the composition too.
7. **See it.** `olr-cli level preview <pack> --out preview.png --scale 2`
   draws the level with its spawns marked (`--material` draws the
   material instead). `olr-cli render` draws a frame of a scenario.
8. **Import OLX content.** `olr-cli import --olx <OpenLieroX share/gamedir>
   --out <dir> --all` converts OLX mods, levels, themes, skins and the
   core files into packs, offline.
