//! Prints the world checksum stream of replays, to compare platforms
//! (networking spec §9.5): native, WASI (`tools/wasi-run.mjs`), Windows and
//! arm64 under QEMU. Replays carry keys only, so the bots' own maths is not
//! under test (bots think on the host only).

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;

#[derive(Parser)]
#[command(name = "rl-detcheck", about = "Prints replays' checksum streams")]
struct Args {
    /// Imported content (packs).
    #[arg(long, default_value = "content/packs")]
    content: PathBuf,
    /// A line every this many ticks (tick 0 and the last tick always).
    #[arg(long, default_value_t = 10)]
    every: u32,
    replays: Vec<PathBuf>,
}

fn main() -> ExitCode {
    match run(&Args::parse()) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::FAILURE
        }
    }
}

/// Prints `<file name> <tick> <checksum hex>`; false if a replay's own
/// recorded checksums do not match.
fn run(a: &Args) -> anyhow::Result<bool> {
    let every = a.every.max(1);
    let content = rl_content::Content::open(&a.content)?;
    let mut ok = true;
    for path in &a.replays {
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let replay = rl_replay::Replay::decode(&std::fs::read(path)?)
            .map_err(|e| anyhow::anyhow!("{}: {e:?}", path.display()))?;
        let mut world = content.world_for_replay(&replay)?;
        let mut events = rl_sim::TickEvents::new();
        let mut cursor = rl_replay::Cursor::default();
        println!("{name} 0 {:016x}", world.checksum());
        let mut tick = 0u32;
        loop {
            match cursor.step(&replay, &mut world, &mut events) {
                Ok(true) => {}
                Ok(false) => break,
                Err(m) => {
                    eprintln!("{name}: {m:?}");
                    ok = false;
                    break;
                }
            }
            tick += 1;
            if tick.is_multiple_of(every) || tick == replay.ticks() {
                println!("{name} {tick} {:016x}", world.checksum());
            }
        }
    }
    Ok(ok)
}
