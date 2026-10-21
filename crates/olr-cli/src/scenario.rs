use std::path::PathBuf;

/// Replays oracle scenarios in the simulation and compares them with OLX.
#[derive(clap::Args)]
pub struct Args {
    /// Scenario names (`<fixtures>/<name>.ron`); all if none.
    names: Vec<String>,
    /// Imported content (packs written by `olr-cli import`), OLR's own packs
    /// (the `reloaded` directory beside it) over it.
    #[arg(long, default_value = "content/packs")]
    content: PathBuf,
    /// Directory with the scenarios and OLX's traces.
    #[arg(long, default_value = "fixtures/oracle")]
    fixtures: PathBuf,
    /// Print OLX's and the simulation's state at this frame as JSON lines.
    #[arg(long)]
    dump: Option<i64>,
    /// Compare every trace field, not only those the simulation reproduces so far
    /// (`olr_scenario::CHECKED`).
    #[arg(long, conflicts_with = "fields")]
    all: bool,
    /// Compare these field groups, `+`-separated, such as
    /// `checked+projectiles+proj_hidden+clocks` (`checked` is `olr_scenario::CHECKED`).
    #[arg(long)]
    fields: Option<String>,
    /// Time every frame in olr-sim and write the times as RON here (plan 8;
    /// a fixture with a header-only trace is timed without comparing).
    #[arg(long, conflicts_with_all = ["dump", "all", "fields"])]
    time: Option<PathBuf>,
}

/// Parses `--fields`.
fn parse_fields(s: &str) -> anyhow::Result<olr_oracle::Fields> {
    use olr_oracle::Fields;
    let mut out = Fields::NONE;
    for name in s.split('+') {
        out = out.union(match name {
            "checked" => olr_scenario::CHECKED,
            "all" => Fields::ALL,
            "worm_motion" => Fields::WORM_MOTION,
            "slots" => Fields::SLOTS,
            "rope" => Fields::ROPE,
            "material" => Fields::MATERIAL,
            "rand" => Fields::RAND,
            "shots" => Fields::SHOTS,
            "projectiles" => Fields::PROJECTILES,
            "proj_hidden" => Fields::PROJ_HIDDEN,
            "health" => Fields::HEALTH,
            "score" => Fields::SCORE,
            "respawn" => Fields::RESPAWN,
            "bonuses" => Fields::BONUSES,
            "clocks" => Fields::CLOCKS,
            "game_over" => Fields::GAME_OVER,
            other => anyhow::bail!("unknown field group {other:?}"),
        });
    }
    Ok(out)
}

/// Returns `Ok(false)` if any scenario diverges from OLX.
pub fn run(args: &Args) -> anyhow::Result<bool> {
    let names =
        if args.names.is_empty() { olr_scenario::fixture_names(&args.fixtures)? } else { args.names.clone() };
    let fields = match (&args.fields, args.all) {
        (Some(f), _) => parse_fields(f)?,
        (None, true) => olr_oracle::Fields::ALL,
        (None, false) => olr_scenario::CHECKED,
    };
    let content = olr_content::Content::open_bundled(&args.content)?;
    if let Some(out) = &args.time {
        let mut report = olr_cli::timing::TimingReport::default();
        let mut ok = true;
        for name in &names {
            let (sc, expected) = olr_scenario::load_fixture(&args.fixtures, name)?;
            let t = olr_cli::timing::time_scenario(&content, name, &sc, &expected)?;
            // Nearest rank, as every percentile in plan 8 (ruling PF8).
            let us: Vec<f64> = t.tick_ns.iter().map(|&ns| ns as f64 / 1000.0).collect();
            println!(
                "time {name}: {} frames, tick p50 {:.1} µs, peak {} projectiles, matches OLX: {:?}",
                us.len(),
                olr_cli::stress::Spread::of(&us).p50,
                t.projectiles.iter().max().copied().unwrap_or(0),
                t.matches_olx
            );
            ok &= t.matches_olx != Some(false);
            report.scenarios.push(t);
        }
        std::fs::write(out, ron::ser::to_string_pretty(&report, ron::ser::PrettyConfig::default())? + "\n")?;
        return Ok(ok);
    }
    let mut ok = true;
    for name in names {
        let (sc, expected) = olr_scenario::load_fixture(&args.fixtures, &name)?;
        let actual = olr_scenario::replay(&content, &sc, &expected)?;
        match olr_oracle::compare_fields(&expected, &actual, &sc.tolerances, fields) {
            None => println!("ok   {name}: {} frames match OLX", expected.frames.len()),
            Some(d) => {
                ok = false;
                println!("DIFF {name}: {d}");
            }
        }
        if let Some(f) = args.dump {
            for (who, trace) in [("olx", &expected), ("sim", &actual)] {
                let frame = trace.frame(f).ok_or_else(|| anyhow::anyhow!("{name} has no frame {f}"))?;
                println!("{who} {}", serde_json::to_string(frame)?);
            }
        }
    }
    Ok(ok)
}
