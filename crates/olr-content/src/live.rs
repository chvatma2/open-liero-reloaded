//! A level's live copies as a match loads them (issue #85): the played
//! form as read (`olr_format::levels::LiveSource`: pictures, masks,
//! scripts and sounds, for presentation) and the simulation's data made
//! from it once ([`olr_sim::LiveLevel`], shared by an `Arc`).

use std::sync::Arc;

use olr_format::levels::{LiveFile, LiveSource, PartKind, Target};
use olr_sim::{
    LiveCopy, LiveCopySetup, LiveHealth, LiveLevel, LivePart, LivePartKind, LiveProp, LiveSolid, LiveState,
    LiveTarget,
};

/// A level's live copies: the played form and the simulation's data.
#[derive(Clone, Debug)]
pub struct LoadedLive {
    /// `live.ron` and `live/` as read (checked: `check_live`).
    pub source: LiveSource,
    /// The simulation's data, made from `source`.
    pub sim: Arc<LiveLevel>,
}

impl LoadedLive {
    /// The live copies of a checked played form.
    pub fn new(source: LiveSource) -> LoadedLive {
        let sim = Arc::new(live_level(&source));
        LoadedLive { source, sim }
    }
}

/// `t` as the simulation names it.
fn target(t: Target) -> LiveTarget {
    use LiveTarget as L;
    use Target as T;
    match t {
        T::X => L::X,
        T::Y => L::Y,
        T::Visible => L::Visible,
        T::Facing => L::Facing,
        T::Health => L::Health,
        T::OffsetX(l) => L::OffsetX(l),
        T::OffsetY(l) => L::OffsetY(l),
        T::LayerVisible(l) => L::LayerVisible(l),
        T::Opacity(l) => L::Opacity(l),
        T::GlowStrength(l) => L::GlowStrength(l),
        T::GlowColour(l) => L::GlowColour(l),
        T::LightIntensity(l) => L::LightIntensity(l),
        T::LightReach(l) => L::LightReach(l),
        T::GlowAnimation(l) => L::GlowAnimation(l),
        T::BeamDirection(l) => L::BeamDirection(l),
        T::BeamSpread(l) => L::BeamSpread(l),
        T::GlassOpacity(l) => L::GlassOpacity(l),
        T::GlassTint(l) => L::GlassTint(l),
    }
}

/// The simulation's data of played form `src`: each copy's values,
/// states, parts, solid masks (a picture's `-rock.png`, 0 open), start,
/// health, scripts and sounds. A mask shorter than its picture (the
/// reader refuses one) is open where it is missing.
pub fn live_level(src: &LiveSource) -> LiveLevel {
    let LiveFile { copies, scripts: _, level_script, sounds } = &src.file;
    let copies = copies
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let pictures = src.pictures.get(i).map_or(&[][..], Vec::as_slice);
            let solids = c
                .pictures
                .iter()
                .enumerate()
                .map(|(k, p)| {
                    let rock = pictures.get(k).and_then(|x| x.rock.as_deref());
                    let w = p.size.0 as usize;
                    LiveSolid::new(p.size, rock.is_some(), |x, y| {
                        rock.and_then(|r| r.get(y as usize * w + x as usize)).is_some_and(|&v| v != 0)
                    })
                })
                .collect();
            let states = c
                .states
                .iter()
                .map(|s| LiveState {
                    name: s.name.clone(),
                    values: s.values.clone(),
                    parts: s
                        .parts
                        .iter()
                        .map(|p| LivePart {
                            kind: match p.kind {
                                PartKind::Rock => LivePartKind::Rock,
                                PartKind::Background => LivePartKind::Background,
                            },
                            layer: p.layer,
                            at: p.at,
                            picture: p.picture,
                            glow_strength: p.glow_strength,
                        })
                        .collect(),
                    enter_sound: s.enter_sound,
                    loop_sound: s.loop_sound,
                })
                .collect();
            LiveCopy::new(LiveCopySetup {
                id: c.id.clone(),
                at: c.at,
                size: c.size,
                props: c
                    .props
                    .iter()
                    .map(|p| LiveProp {
                        name: p.name.clone(),
                        target: target(p.drives),
                        min: p.min,
                        max: p.max,
                        default: p.default,
                    })
                    .collect(),
                states,
                pictures: solids,
                start_state: c.start_state,
                start_values: c.start_values.clone(),
                health: c.health.map(|h| LiveHealth { points: h.points, at_zero: h.at_zero }),
                scripts: c.scripts.clone(),
                sounds: c.sounds.clone(),
            })
        })
        .collect();
    let sounds = u16::try_from(sounds.len()).unwrap_or(u16::MAX);
    LiveLevel::new(copies, src.scripts.clone(), *level_script, sounds)
}
