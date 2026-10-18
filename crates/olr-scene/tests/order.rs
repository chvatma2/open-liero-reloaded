//! The draw list's layers and the order inside a worm follow OLX's
//! `CViewport::gusRender` and `CWorm::Draw` (presentation note §11.2, §3).

use crate::common;

use std::collections::BTreeSet;

use olr_data::image::Rgba;
use olr_scene::{AtlasId, DrawList, Interpolation, Item, Layer, Rect, SceneAssets, SceneOptions, Sprite};

/// What a worm-layer item is, in `CWorm::Draw`'s order.
fn worm_rank(a: &SceneAssets, s: &Sprite, skins: &BTreeSet<AtlasId>) -> u8 {
    match s.atlas {
        id if id == a.white => 0, // rope pixels
        id if id == a.hook => 1,
        id if id == a.crosshair => 2,
        id if skins.contains(&id) => 3,
        id if id == a.muzzle => 4,
        id => panic!("unexpected image {:?} in the worm layer", a.image(id).name),
    }
}

fn check(list: &DrawList, a: &SceneAssets, alive: usize, seen: &mut BTreeSet<&'static str>) {
    let order: Vec<Layer> = list.items().map(|(l, _)| l).collect();
    assert!(order.is_sorted(), "items come layer by layer");
    assert!(list.layer(Layer::Entities).is_empty(), "entities are the caller's");
    for item in list.layer(Layer::Beams) {
        match item {
            Item::Line(_) => seen.insert("beam"),
            Item::Sprite(s) if s.atlas == a.white => seen.insert("laser"),
            Item::Sprite(s) => panic!("{} in the beam layer", a.image(s.atlas).name),
        };
    }
    for item in list.layer(Layer::Projectiles) {
        let Item::Sprite(s) = item else { panic!("a line among projectiles") };
        assert!(a.image(s.atlas).name.starts_with("mod:") || s.atlas == a.white);
        seen.insert("projectile");
    }
    for item in list.layer(Layer::Bonuses) {
        let Item::Sprite(s) = item else { panic!("a line among bonuses") };
        assert!(s.atlas == a.bonus || s.atlas == a.health);
        seen.insert("bonus");
    }
    let skins: BTreeSet<AtlasId> = (0..alive).map(|i| a.worm(i).skin).collect();
    let (mut last, mut worms) = (None, 0);
    for item in list.layer(Layer::Worms) {
        let Item::Sprite(s) = item else { panic!("a line among worms") };
        let r = worm_rank(a, s, &skins);
        seen.insert(["rope", "hook", "crosshair", "skin", "muzzle"][r as usize]);
        worms += usize::from(r == 3);
        // Each worm: rope, hook, crosshair, skin, muzzle; the next worm starts over.
        assert!(last.is_none_or(|l| r >= l || l >= 3), "worm items out of order");
        last = Some(r);
    }
    assert!(worms <= alive);
}

#[test]
fn draw_list_order_matches_olx_layers() {
    assert_eq!(Layer::ALL, [Layer::Entities, Layer::Beams, Layer::Projectiles, Layer::Bonuses, Layer::Worms]);
    let Some(content) = common::content() else { return };
    let latest = SceneOptions { interpolation: Interpolation::Latest, ..SceneOptions::default() };
    let mut seen = BTreeSet::new();
    let mut list = DrawList::new();
    for name in
        ["rope", "fire-minigun", "bonus-weapon", "beam-burst", "weapons/classic/rifle", "two-worms-fire"]
    {
        common::play(&content, name, latest, |world, _, _, scene, assets| {
            scene.build(world, 1.0, assets, &mut list);
            let alive = world.worms().iter().filter(|w| w.alive).count();
            check(&list, assets, alive, &mut seen);
        });
    }
    let all = ["beam", "laser", "projectile", "bonus", "rope", "hook", "crosshair", "skin", "muzzle"];
    assert_eq!(seen, all.into_iter().collect(), "every kind of item was drawn");

    // The caller's entities go first, whenever they are appended.
    let first = list.items().next().map(|(l, _)| l);
    let src = Rect { x: 0, y: 0, w: 1, h: 1 };
    let white = Rgba::rgb(255, 255, 255);
    let entity = Sprite {
        atlas: AtlasId(0),
        src,
        pos: [0.0, 0.0],
        size: [1.0, 1.0],
        flip: false,
        tint: white,
        shade: Default::default(),
    };
    list.sprite(Layer::Entities, entity);
    assert_ne!(first, Some(Layer::Entities));
    assert_eq!(list.items().next(), Some((Layer::Entities, &Item::Sprite(entity))));
}
