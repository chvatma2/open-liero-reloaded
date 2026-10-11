use std::path::PathBuf;

/// Replays oracle scenarios in the simulation and compares them with OLX.
#[derive(clap::Args)]
pub struct Args {
    /// Scenario names (`<fixtures>/<name>.ron`); all if none.
    names: Vec<String>,
    /// Imported content (packs written by `rl-cli import`).
    #[arg(long, default_value = "content/packs")]
    content: PathBuf,
    /// Directory with the scenarios and OLX's traces.
    #[arg(long, default_value = "fixtures/oracle")]
    fixtures: PathBuf,
    /// Print OLX's and the simulation's state at this frame as JSON lines.
    #[arg(long)]
    dump: Option<i64>,
    /// Compare every trace field, not only those the simulation reproduces so far
    /// (`rl_scenario::CHECKED`).
    #[arg(long, conflicts_with = "fields")]
    all: bool,
    /// Compare these field groups, `+`-separated, such as
    /// `checked+projectiles+proj_hidden+clocks` (`checked` is `rl_scenario::CHECKED`).
    #[arg(long)]
    fields: Option<String>,
}

/// Parses `--fields`.
fn parse_fields(s: &str) -> anyhow::Result<rl_oracle::Fields> {
    use rl_oracle::Fields;
    let mut out = Fields::NONE;
    for name in s.split('+') {
        out = out.union(match name {
            "checked" => rl_scenario::CHECKED,
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
        if args.names.is_empty() { rl_scenario::fixture_names(&args.fixtures)? } else { args.names.clone() };
    let fields = match (&args.fields, args.all) {
        (Some(f), _) => parse_fields(f)?,
        (None, true) => rl_oracle::Fields::ALL,
        (None, false) => rl_scenario::CHECKED,
    };
    let content = rl_content::Content::open(&args.content)?;
    let mut ok = true;
    for name in names {
        let (sc, expected) = rl_scenario::load_fixture(&args.fixtures, &name)?;
        let actual = rl_scenario::replay(&content, &sc, &expected)?;
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
