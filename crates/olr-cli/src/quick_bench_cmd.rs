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
    println!(
        "lit frame:   {} instructions (uploads and encoding, fewest of {}; {})",
        show(report.lit_frame_instructions),
        quick_bench::LIT_FRAMES,
        quick_bench::LIT_LEVEL
    );
    match report.lit_frame_ms_min {
        Some(ms) => println!(
            "lit frame:   {ms:.2} ms (fastest of {}, {} glowing discs; software adapter: relative only)",
            quick_bench::LIT_FRAMES,
            report.lit_discs.unwrap_or(0)
        ),
        None => println!("lit frame:   not measured"),
    }
    if let Some(bytes) = report.lit_frame_upload_bytes {
        println!("lit frame:   {bytes} bytes uploaded");
    }
    if let Some(passes) = report.lit_passes {
        let groups: Vec<String> = report.lit_pass_ms.iter().map(|(g, ms)| format!("{g} {ms:.2}")).collect();
        println!("lit frame:   {passes} passes; fastest ms per group: {}", groups.join(", "));
    }
    println!(
        "terrain:     {} instructions, {} texture writes, {} bytes ({} ticks' uploads)",
        show(report.terrain_upload_instructions),
        show(report.terrain_upload_writes),
        show(report.terrain_upload_bytes),
        quick_bench::TERRAIN_TICKS
    );
    if let Some(path) = &args.out {
        let text = ron::ser::to_string_pretty(&report, ron::ser::PrettyConfig::default())?;
        std::fs::write(path, text + "\n")?;
    }
    Ok(true)
}
