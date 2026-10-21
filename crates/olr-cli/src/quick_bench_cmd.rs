//! `olr-cli quick-bench`: `olr_cli::quick_bench`'s numbers printed and,
//! with `--out`, written as RON (`cargo xtask bench-quick` reads it).

use std::path::PathBuf;

use olr_cli::quick_bench;

#[derive(clap::Args)]
pub struct Args {
    /// Imported content (packs written by `olr-cli import`), OLR's own packs
    /// (the `reloaded` directory beside it) over it.
    #[arg(long, default_value = "content/packs")]
    content: PathBuf,
    /// Write the report as RON.
    #[arg(long)]
    out: Option<PathBuf>,
}

pub fn run(args: &Args) -> anyhow::Result<bool> {
    let content = olr_content::Content::open_bundled(&args.content)?;
    let report = quick_bench::run(&content, &mut |line| eprintln!("notice: {line}"))?;
    let show = |v: Option<u64>| v.map_or("not measured".into(), |n| n.to_string());
    println!(
        "match ticks: {} instructions ({} ticks)",
        show(report.match_ticks_instructions),
        quick_bench::MATCH_TICKS
    );
    println!(
        "draw lists:  {} instructions ({} builds)",
        show(report.draw_lists_instructions),
        quick_bench::DRAW_LISTS
    );
    match report.render_frame_ms_min {
        Some(ms) => println!("1080p frame: {ms:.2} ms (fastest of {})", quick_bench::FRAMES),
        None => println!("1080p frame: not measured"),
    }
    if let Some(path) = &args.out {
        let text = ron::ser::to_string_pretty(&report, ron::ser::PrettyConfig::default())?;
        std::fs::write(path, text + "\n")?;
    }
    Ok(true)
}
