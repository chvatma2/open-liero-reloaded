//! A level's live copies as a match loads them (issue #85): the played
//! form as read (`olr_format::levels::LiveSource`: pictures, masks,
//! scripts and sounds, for presentation) and the simulation's data made
//! from it once ([`olr_sim::LiveLevel`], shared by an `Arc`); issue #87:
//! each copy's glow groups compiled and its pictures as the colour map
//! draws them ([`olr_terrain::LiveArt`]), the copy's groups numbered after
//! the level's; issue #88: its scripts compiled once
//! ([`olr_script::director`]), a script that does not compile refusing
//! the level.

use std::sync::Arc;

use olr_format::levels::{LIVE_FILE, LiveFile, LiveSource, PartKind, Target};
use olr_format::look::{LookSource, compile_look_source};
use olr_look::GlowGroup;
use olr_sim::{
    LiveCopy, LiveCopySetup, LiveHealth, LiveLevel, LivePart, LivePartKind, LiveProp, LiveSolid, LiveState,
    LiveTarget, LiveView,
};
use olr_terrain::{LiveArt, LiveFrame, LivePicture, LivePlaced};

/// A level's live copies: the played form, the simulation's data and
/// what presentation draws.
#[derive(Clone, Debug)]
pub struct LoadedLive {
    /// `live.ron` and `live/` as read (checked: `check_live`).
    pub source: LiveSource,
    /// The simulation's data, made from `source`.
    pub sim: Arc<LiveLevel>,
    /// Each copy's own glow groups, compiled (issue #87); a copy whose
    /// groups pass the 255 group map values has none.
    pub groups: Vec<Vec<GlowGroup>>,
    /// Every copy's pictures as the colour map draws them (issue #87),
    /// their groups numbered after the level's.
    pub art: Arc<LiveArt>,
    /// What loading left out (a copy drawn without glow), to log once.
    pub warnings: Vec<String>,
    /// Its scripts, compiled (issue #88), shared by every world of a
    /// match; `None` when no copy and no level script runs one.
    pub director: Option<Box<dyn olr_sim::Director>>,
}

impl LoadedLive {
    /// The live copies of a checked played form in a level of
    /// `level_groups` glow groups (its look's).
    ///
    /// The group map is one byte: the level's groups keep `1..=n`, then
    /// copy `c`'s group `g` is `n + offset(c) + g`, copies in order (the
    /// design's call 13). The bake refuses more than 255 together; a copy
    /// past them all the same is drawn without glow, with a warning, never
    /// a panic. A script that does not compile refuses it (issue #88:
    /// every peer refuses the same bytes alike).
    pub fn new(source: LiveSource, level_groups: usize) -> anyhow::Result<LoadedLive> {
        let sim = Arc::new(live_level(&source));
        let director = olr_script::director(&sim).map_err(|e| {
            let at = source.file.scripts.get(usize::from(e.script)).map_or("a script", String::as_str);
            anyhow::anyhow!("{LIVE_FILE}: {at}: {}", e.issue)
        })?;
        let mut warnings = Vec::new();
        let mut next = level_groups;
        let mut groups = Vec::with_capacity(source.file.copies.len());
        let mut art = LiveArt { copies: Vec::with_capacity(source.file.copies.len()) };
        for (i, c) in source.file.copies.iter().enumerate() {
            let look = LookSource { glow_groups: c.groups.clone(), ..LookSource::default() };
            let own = compile_look_source(&look, LIVE_FILE).map(|l| l.groups).unwrap_or_default();
            let base = next;
            next += c.groups.len();
            let fits = next <= usize::from(u8::MAX) && own.len() == c.groups.len();
            if !fits {
                warnings.push(format!(
                    "live copy {}: its glow groups pass the level's 255: drawn without glow",
                    c.id
                ));
            }
            let pictures = source.pictures.get(i).map_or(&[][..], Vec::as_slice);
            art.copies.push(
                c.pictures
                    .iter()
                    .enumerate()
                    .map(|(k, entry)| {
                        let Some(p) = pictures.get(k) else { return LivePicture::default() };
                        LivePicture {
                            size: entry.size,
                            art: p.art.as_raw().to_vec(),
                            rock: p.rock.clone(),
                            glow: p.glow.clone().filter(|_| fits),
                            groups: p.groups.as_ref().filter(|_| fits).map(|g| {
                                let value = |v: u8| match usize::from(v) {
                                    0 => 0,
                                    v if v <= c.groups.len() => (base + v) as u8,
                                    _ => 0,
                                };
                                g.iter().map(|&v| value(v)).collect()
                            }),
                        }
                    })
                    .collect(),
            );
            groups.push(if fits { own } else { Vec::new() });
        }
        Ok(LoadedLive { source, sim, groups, art: Arc::new(art), warnings, director })
    }
}

/// Fills `frame` with the parts `live` shows now (issue #87), copy by
/// copy, for `ColorMap::set_live`: each visible part at its whole-pixel
/// place, mirrored within its copy's rectangle when the copy's `Facing` is
/// not 0, a background part at its layer's `Opacity` (percent, as the
/// bake rounds it to an alpha), each at its layer's `GlowStrength`, else
/// its strength as authored. Allocates nothing once `frame` is warm.
pub fn live_frame(live: LiveView<'_>, frame: &mut LiveFrame) {
    frame.clear();
    for c in live.copies() {
        let (rx, _, w, _) = c.rect();
        let flip = c.target(LiveTarget::Facing).is_some_and(|v| v != 0);
        let pictures = &c.setup().pictures;
        for (p, (x, y)) in c.visible_parts() {
            let layer = |t: fn(u16) -> LiveTarget| p.layer.and_then(|l| c.target(t(l)));
            let x = if flip {
                let pw = pictures.get(usize::from(p.picture)).map_or(0, |s| i64::from(s.size.0));
                let m = 2 * i64::from(rx) + i64::from(w) - i64::from(x) - pw;
                m.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
            } else {
                x
            };
            let rock = p.kind == LivePartKind::Rock;
            let opacity = if rock { 100 } else { layer(LiveTarget::Opacity).unwrap_or(100).clamp(0, 100) };
            let glow = layer(LiveTarget::GlowStrength).unwrap_or(i64::from(p.glow_strength)).clamp(0, 100);
            frame.push(LivePlaced {
                picture: p.picture,
                rock,
                at: (x, y),
                flip,
                alpha: ((opacity as u32 * 255 + 50) / 100) as u8,
                glow: glow as u8,
            });
        }
        frame.end_copy();
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
