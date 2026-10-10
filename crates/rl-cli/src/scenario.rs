use std::path::PathBuf;

/// Replays oracle scenarios in the simulation and compares them with OLX.
#[derive(clap::Args)]
pub struct Args {
    /// Scenario names (`<fixtures>/<name>.ron`); all if none.
    names: Vec<String>,
    /// Imported content (packs written by `rl-cli import`).
    #[arg(long, default_value = "target/content")]
    content: PathBuf,
    /// Directory with the scenarios and OLX's traces.
    #[arg(long, default_value = "fixtures/oracle")]
    fixtures: PathBuf,
    /// Print OLX's and the simulation's state at this frame as JSON lines.
    #[arg(long)]
    dump: Option<i64>,
    /// Compare every trace field, not only those the simulation reproduces so far
    /// (`rl_scenario::CHECKED`).
    #[arg(long)]
    all: bool,
}

/// Returns `Ok(false)` if any scenario diverges from OLX.
pub fn run(args: &Args) -> anyhow::Result<bool> {
    let names =
        if args.names.is_empty() { rl_scenario::fixture_names(&args.fixtures)? } else { args.names.clone() };
    let mut ok = true;
    for name in names {
        let (sc, expected) = rl_scenario::load_fixture(&args.fixtures, &name)?;
        let actual = rl_scenario::replay(&args.content, &sc, &expected)?;
        let fields = if args.all { rl_oracle::Fields::ALL } else { rl_scenario::CHECKED };
        match rl_oracle::compare_fields(&expected, &actual, &sc.tolerances, fields) {
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
