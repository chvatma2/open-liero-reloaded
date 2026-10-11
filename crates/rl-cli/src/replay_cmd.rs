//! `rl-cli replay`: plays a recorded match, checking its checksums (spec §7.3).

use std::path::PathBuf;

use rl_replay::Replay;
use rl_sim::{TickEvents, World};
use serde_json::json;

/// Plays a replay on the content it was recorded with, checking every
/// recorded checksum; exits with status 1 at the first mismatch.
#[derive(clap::Args)]
pub struct Args {
    /// A replay (`rl-cli match --record`, or one in `target/failures/`).
    file: PathBuf,
    /// Imported content (packs written by `rl-cli import`).
    #[arg(long, default_value = "content/packs")]
    content: PathBuf,
    /// Stop after this many ticks (the world before tick N, 0-based).
    #[arg(long)]
    until: Option<u32>,
    /// Print the world where it stopped as JSON.
    #[arg(long)]
    dump: bool,
}

/// Returns `Ok(false)` if the replay diverges.
pub fn run(args: &Args) -> anyhow::Result<bool> {
    let bytes = std::fs::read(&args.file)?;
    let replay = Replay::decode(&bytes).map_err(anyhow::Error::new)?;
    let content = rl_content::Content::open(&args.content)?;
    let mut world = content.world_for_replay(&replay)?;
    let until = args.until.unwrap_or(u32::MAX).min(replay.ticks());
    let mut player = replay.player();
    let mut out = TickEvents::new();
    let mut ok = true;
    while player.tick() < until {
        match player.step(&mut world, &mut out) {
            Ok(true) => {}
            Ok(false) => break,
            Err(m) => {
                println!("MISMATCH {m}");
                ok = false;
                break;
            }
        }
    }
    if ok {
        println!(
            "ok   tick {} of {}: {} recorded checksums match",
            player.tick(),
            replay.ticks(),
            player.checked()
        );
    }
    if args.dump {
        println!("tick {}", player.tick());
        println!("{}", serde_json::to_string_pretty(&dump(&world))?);
    }
    Ok(ok)
}

/// The world's state as JSON: clocks, checksum, result, scores, worms.
fn dump(w: &World) -> serde_json::Value {
    let worms: Vec<_> = w
        .worms()
        .iter()
        .map(|m| {
            json!({
                "id": m.id,
                "alive": m.alive,
                "pos": [m.pos.x, m.pos.y],
                "vel": [m.vel.x, m.vel.y],
                "health": m.health,
                "angle": m.angle,
                "face": format!("{:?}", m.face),
                "current_weapon": m.current_weapon,
                "slots": m.slots.iter().map(|s| json!({
                    "weapon": s.weapon, "charge": s.charge, "reloading": s.reloading
                })).collect::<Vec<_>>(),
                "rope": {
                    "shooting": m.rope.shooting,
                    "attached": m.rope.attached,
                    "player_attached": m.rope.player_attached,
                    "pos": [m.rope.pos.x, m.rope.pos.y],
                },
                "lives": m.lives, "kills": m.kills, "deaths": m.deaths, "suicides": m.suicides,
            })
        })
        .collect();
    let scores: Vec<_> = w
        .scores()
        .iter()
        .map(|s| json!({ "worm": s.worm, "score": s.score, "lives": s.lives, "kills": s.kills, "deaths": s.deaths }))
        .collect();
    json!({
        "server_frame": w.server_frame(),
        "time_ms": w.time().0,
        "checksum": format!("{:#018x}", w.checksum()),
        "result": w.result().map(|r| format!("{r:?}")),
        "scores": scores,
        "projectiles": w.projectiles().count(),
        "bonuses": w.bonuses().iter().filter(|b| b.used).count(),
        "entities": w.entity_count(),
        "worms": worms,
    })
}
