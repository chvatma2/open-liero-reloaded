//! Prints the world checksum stream of replays, to compare platforms
//! (networking spec §9.5): native, WASI (`tools/wasi-run.mjs`), Windows and
//! arm64 under QEMU. Replays carry keys only, so the bots' own maths is not
//! under test (bots think on the host only). `--bake` adds the digest of a
//! composed level's bake (plan 14a): the image pipeline and the bake must
//! give the same bytes everywhere too.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;

#[derive(Parser)]
#[command(name = "olr-detcheck", about = "Prints replays' checksum streams")]
struct Args {
    /// Imported content (packs).
    #[arg(long, default_value = "content/packs")]
    content: PathBuf,
    /// A line every this many ticks (tick 0 and the last tick always).
    #[arg(long, default_value_t = 10)]
    every: u32,
    /// A composed level pack: prints `bake <sha256 hex>` of its bake, after
    /// the replays' lines.
    #[arg(long)]
    bake: Option<PathBuf>,
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
    let mut ok = true;
    if a.replays.is_empty() {
        if let Some(pack) = &a.bake {
            println!("bake {}", bake_digest(&a.content, pack)?);
        }
        return Ok(ok);
    }
    let content = olr_content::Content::open(&a.content)?;
    for path in &a.replays {
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let replay = olr_replay::Replay::decode(&std::fs::read(path)?)
            .map_err(|e| anyhow::anyhow!("{}: {e:?}", path.display()))?;
        let mut world = content.world_for_replay(&replay)?;
        let mut events = olr_sim::TickEvents::new();
        let mut cursor = olr_replay::Cursor::default();
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
    if let Some(pack) = &a.bake {
        println!("bake {}", bake_digest(&a.content, pack)?);
    }
    Ok(ok)
}

/// SHA-256 over `material.png`, `front.png` and `back.png` as the bake of
/// the composed level at `pack` gives them (encoded by
/// `write_level_files`), in that order, each prefixed with its length as
/// 8 bytes little-endian; the back tile from `content`'s theme pack.
fn bake_digest(content: &std::path::Path, pack: &std::path::Path) -> anyhow::Result<String> {
    use olr_vfs::ContentSource as _;
    use sha2::Digest as _;
    let fail = |e: &dyn std::fmt::Display| anyhow::anyhow!("{}: {e}", pack.display());
    let p = olr_edit::pack::load(pack).map_err(|e| fail(&e))?;
    let (olr_edit::pack::Source::Level(level), Some(src)) = (&p.source, &p.composition) else {
        anyhow::bail!("{}: not a composed level pack", pack.display());
    };
    let tile = olr_edit::compose::theme_back_tile(content, &level.theme).map_err(|e| fail(&e))?;
    let mut baked = level.clone();
    olr_edit::compose::bake(&mut baked, src, &tile).map_err(|e| fail(&e))?;
    let mut fs = olr_vfs::MemFs::new();
    olr_format::levels::write_level_files(&mut fs, &baked).map_err(|e| fail(&e))?;
    let mut h = sha2::Sha256::new();
    for file in ["material.png", "front.png", "back.png"] {
        let bytes = fs.read(&olr_vfs::VPath::new(file)?)?;
        h.update((bytes.len() as u64).to_le_bytes());
        h.update(&bytes);
    }
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}
