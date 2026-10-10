//! Navigation on synthetic levels: OLX's area growth, its passage rule, and
//! the resumable search (notes §1.4, §4.3, §4.8).

mod common;

use common::*;
use rl_bots::Budget;
use rl_bots::nav::{AreaCache, Grower, Rect, Search, SearchError, SearchStatus, trace_worm_line};
use rl_data::level::Material;
use rl_math::Vec2;
use rl_terrain::Terrain;

fn grow(t: &Terrain, p: (i32, i32)) -> Rect {
    Grower::new(p, t.width(), t.height()).step(t, &mut Budget::new(u32::MAX)).unwrap()
}

/// Searches to the end with an unlimited budget: the status, the path and
/// the units spent.
fn search(
    t: &Terrain,
    cache: &std::sync::Arc<AreaCache>,
    from: (i32, i32),
    to: (i32, i32),
) -> (SearchStatus, Vec<Vec2>, u32) {
    let mut s = Search::new(cache.clone());
    s.start(from, to);
    let mut b = Budget::new(u32::MAX);
    let status = s.step(t, &mut b);
    (status, s.path().to_vec(), b.used())
}

/// Two rooms 5..=39 and 43..=84 (x) by 5..=54 (y), split by a 3 px rock
/// wall with a gap of `w` rows from `top`.
fn two_rooms(top: i32, w: i32) -> Terrain {
    rocks(90, 60, |x, y| {
        let border = !inside(x, y, (5, 5, 84, 54));
        let wall = (40..=42).contains(&x) && !(top..top + w).contains(&y);
        border || wall
    })
}

/// A U: two arms 10..=29 and 70..=89 (x) by 10..=69 (y), joined below 50.
fn u_shape() -> Terrain {
    rocks(100, 80, |x, y| {
        !(inside(x, y, (10, 10, 29, 69)) || inside(x, y, (70, 10, 89, 69)) || inside(x, y, (10, 50, 89, 69)))
    })
}

/// Every node is free of rock and at most 50 px from the next on each axis
/// and, if `lines`, a worm line joins each node to the next. (A connector,
/// the centre of two areas' overlap, may sit in a wall's gap so close to
/// its edge that OLX's three-wide worm line grazes the rock.)
fn assert_path(t: &Terrain, path: &[Vec2], lines: bool) {
    for p in path {
        assert_eq!(t.flags(p.x as i64, p.y as i64) & rl_terrain::PX_ROCK, 0, "{p:?} is in rock");
    }
    for s in path.windows(2) {
        assert!(!lines || trace_worm_line(t, s[1], s[0]), "no worm line from {:?} to {:?}", s[0], s[1]);
        let (dx, dy) = ((s[1].x - s[0].x).abs(), (s[1].y - s[0].y).abs());
        assert!(dx <= 50.0 && dy <= 50.0, "a segment over 50 px: {s:?}");
    }
}

#[test]
fn areas_grow_like_olx() {
    // Growth order (`getMaxFreeArea`): down, left, up, right, one pixel
    // each. In an L of a 20 × 5 bar and a 5 × 20 bar, a start in their
    // overlap grows down first, so it keeps the vertical bar.
    let l = rocks(30, 30, |x, y| !(inside(x, y, (0, 0, 19, 4)) || inside(x, y, (0, 0, 4, 19))));
    assert_eq!(grow(&l, (2, 2)), Rect { x0: 0, y0: 0, x1: 4, y1: 19 });
    assert_eq!(grow(&l, (12, 2)), Rect { x0: 0, y0: 0, x1: 19, y1: 4 });
    // Paused after every pixel, it grows the same rectangle.
    let mut g = Grower::new((2, 2), 30, 30);
    let mut steps = 0;
    let r = loop {
        steps += 1;
        if let Some(r) = g.step(&l, &mut Budget::new(1)) {
            break r;
        }
    };
    assert_eq!((r, steps > 100), (Rect { x0: 0, y0: 0, x1: 4, y1: 19 }, true));
    // A free room becomes one area.
    assert_eq!(grow(&two_rooms(28, 6), (20, 50)), Rect { x0: 5, y0: 5, x1: 39, y1: 54 });

    // The passage rule (`check_checkpoint`): checkpoints every 5 px of the
    // left room's edge (rows 7, 12, …, 52) need 7 rock-free traces 5 px
    // deep beside them, the checkpoint's own row not counted. A 6 px gap
    // never passes, an 8 px gap always does, and a 7 px gap only when a
    // checkpoint is on the row just outside it (here row 27 for the gap
    // 28..=34), as in OLX.
    let (from, to) = ((20, 50), (60, 10));
    for top in 25..=29 {
        let t = two_rooms(top, 6);
        let (status, _, _) = search(&t, &AreaCache::new(&t), from, to);
        assert_eq!(status, SearchStatus::Failed(SearchError::NoPath), "6 px gap at {top}");
        let t = two_rooms(top, 8);
        let (status, path, _) = search(&t, &AreaCache::new(&t), from, to);
        assert_eq!(status, SearchStatus::Found, "8 px gap at {top}");
        assert_path(&t, &path, false);
    }
    let t = two_rooms(28, 7);
    let (status, path, _) = search(&t, &AreaCache::new(&t), from, to);
    assert_eq!(status, SearchStatus::Found);
    assert_path(&t, &path, false);
    let t = two_rooms(29, 7);
    assert_eq!(search(&t, &AreaCache::new(&t), from, to).0, SearchStatus::Failed(SearchError::NoPath));

    // A U: the path goes down one arm, along the bottom and up the other,
    // with a node at most every 50 px, and ends at the target.
    let t = u_shape();
    let (status, path, _) = search(&t, &AreaCache::new(&t), (20, 15), (80, 15));
    assert_eq!(status, SearchStatus::Found);
    assert!(path.iter().any(|p| p.y >= 50.0), "under the pillar: {path:?}");
    assert_eq!(path.last(), Some(&Vec2::new(80.0, 15.0)));
    assert_path(&t, &path, true);

    // Dirt does not block paths (the bot digs); the same plug of rock does.
    let plugged = |m: Material| {
        level(120, 40, move |x, y| {
            if inside(x, y, (55, 5, 64, 34)) {
                m
            } else if inside(x, y, (5, 5, 114, 34)) {
                Material::Background
            } else {
                Material::Rock
            }
        })
    };
    let t = plugged(Material::Dirt);
    let (status, path, _) = search(&t, &AreaCache::new(&t), (20, 20), (100, 20));
    assert_eq!(status, SearchStatus::Found);
    assert_eq!(path.last(), Some(&Vec2::new(100.0, 20.0)));
    let t = plugged(Material::Rock);
    assert_eq!(
        search(&t, &AreaCache::new(&t), (20, 20), (100, 20)).0,
        SearchStatus::Failed(SearchError::NoPath)
    );
}

#[test]
fn search_finds_paths_and_resumes_across_ticks() {
    let t = u_shape();
    let (from, to) = ((20, 15), (80, 15));
    let cold = AreaCache::new(&t);
    let (status, whole, cold_units) = search(&t, &cold, from, to);
    assert_eq!(status, SearchStatus::Found);

    // With 300 units a tick the search pauses and resumes until it ends
    // with the same path.
    let cache = AreaCache::new(&t);
    let mut s = Search::new(cache.clone());
    s.start(from, to);
    let mut ticks = 0;
    let mut spent = 0;
    loop {
        ticks += 1;
        let mut b = Budget::new(300);
        let status = s.step(&t, &mut b);
        assert!(b.used() <= 300, "tick {ticks} spent {}", b.used());
        spent += b.used();
        match status {
            SearchStatus::Pending => assert!(ticks < 10_000),
            SearchStatus::Found => break,
            other => panic!("{other:?}"),
        }
    }
    assert!(ticks > 3, "it took {ticks} ticks");
    assert_eq!(s.path(), whole.as_slice());
    assert_eq!(spent, cold_units, "pausing costs nothing extra");
    assert_eq!(cache.len(), cold.len());

    // The areas stay in the cache: searching again reads far less.
    let (status, again, warm_units) = search(&t, &cache, from, to);
    assert_eq!((status, again), (SearchStatus::Found, whole));
    assert!(warm_units * 2 < cold_units, "warm {warm_units}, cold {cold_units}");

    // A search can be restarted at any time, and fails cleanly from rock or
    // outside the map.
    s.start((-5, 3), to);
    assert_eq!(s.step(&t, &mut Budget::new(1000)), SearchStatus::Failed(SearchError::NoStartArea));
    s.start((50, 20), to);
    assert_eq!(s.step(&t, &mut Budget::new(1000)), SearchStatus::Failed(SearchError::NoStartArea));
    s.cancel();
    assert_eq!(s.status(), SearchStatus::Idle);
}
