//! `docs/perf/envelope.md` from a results file (plan 8, spec §7): the
//! method, the machine, every sweep's table and breaking point, each GPU
//! path's frames, shader-probe fits and windowed runs, the OLX comparison
//! and the limits. The headroom summary between [`START`] and [`END`] is
//! written by hand and kept word for word when the report is made again.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use super::results::{self, Budgets, Envelope, GpuPath, OlxComparison, Point, Rollback, Sweep, Windowed};
use crate::root;

pub const REPORT: &str = "docs/perf/envelope.md";
pub const START: &str = "<!-- summary:start -->";
pub const END: &str = "<!-- summary:end -->";
const NO_SUMMARY: &str = "_Not written yet. Write it here; `cargo xtask envelope --report` keeps it._";
/// OLX's projectile pool (`MAX_PROJECTILES`, `include/CProjectile.h:36`).
const OLX_MAX_PROJECTILES: u32 = 3000;

/// The hand-written summary in an existing report.
pub fn summary_of(existing: &str) -> Option<String> {
    let a = existing.find(START)? + START.len();
    let b = existing[a..].find(END)? + a;
    Some(existing[a..b].trim_matches('\n').to_string())
}

fn size(s: (u32, u32)) -> String {
    format!("{}×{}", s.0, s.1)
}

fn ms(us: f64) -> f64 {
    us / 1000.0
}

/// `n thing` or `n things`.
fn count(n: u32, thing: &str) -> String {
    format!("{n} {thing}{}", if n == 1 { "" } else { "s" })
}

/// The present mode in `olr-game`'s adapter line (`<adapter> (<backend>,
/// <type>), <present mode>, <format>`).
fn present_mode(gpu: &str) -> &str {
    gpu.rsplit(", ").nth(1).unwrap_or("—")
}

fn point_row(p: &Point, b: &Budgets) -> String {
    if p.runs == 0 {
        return format!("| {} | — | — | — | — | — | — | — | — | — | — | {} |", p.value, p.caps.join("; "));
    }
    let frame = p.frame_p99_ms().map_or("—".to_string(), |f| format!("{f:.2}"));
    let c = results::causes(p, b, b.frame_144_ms);
    let holds = if c.is_empty() { "yes".to_string() } else { c.join("; ") };
    let sat = if p.saturated.is_empty() { String::new() } else { format!(" ({})", p.saturated.join("; ")) };
    format!(
        "| {} | {:.2} / {:.2} | {:.0} % | {:.2} | {:.2} | {:.2} | {:.2} | {} | {} | {:.0} | {frame} | {holds}{sat} |",
        p.value,
        ms(p.tick_us.p50),
        ms(p.tick_us.p99),
        p.budget_share * 100.0,
        ms(p.sim_us.p99),
        ms(p.think_us.p99),
        ms(p.present_us.p99),
        ms(p.build_us.p99),
        p.projectiles_peak,
        p.fx_peak,
        p.peak_mib
    )
}

fn sweep(o: &mut String, s: &Sweep, b: &Budgets) {
    let _ = writeln!(o, "### {}\n\n{} ({} on {}).\n", s.axis, s.what, s.mod_id, s.level);
    let _ = writeln!(
        o,
        "| value | tick p50 / p99 (ms) | budget used | sim p99 | bots p99 | presentation p99 | draw list p99 | \
         projectiles | effects | MiB | frame with bots p99 (ms) | holds at 144 Hz |\n|---|---|---|---|---|---|---|---|---|---|---|---|"
    );
    for p in &s.points {
        let _ = writeln!(o, "{}", point_row(p, b));
    }
    if let Some(f) = s.points.iter().find_map(|p| p.frame.as_ref()) {
        let _ = writeln!(
            o,
            "\nFrame with bots: the point's level drawn offscreen at {} on {} (`olr-cli gpu-bench`: a tick and a \
             frame; it replays recorded inputs, so its tick has no bots' thinking) plus the point's bots p99 \
             above: a sum of p99s, so a bound. This is the frame the 144 Hz and 60 Hz budgets judge.",
            size(f.size),
            f.path
        );
    }
    let br = |x: &Option<results::Breaking>| {
        x.as_ref().map_or("none: every value holds".to_string(), |b| {
            format!("**{}** ({})", b.value, b.causes.join("; "))
        })
    };
    let _ = writeln!(
        o,
        "\nBreaking point (144 Hz): {}. Breaking point (60 Hz): {}.",
        br(&s.breaking_144),
        br(&s.breaking_60)
    );
    if let Some(f) = s.fit {
        if s.axis == "worms" {
            let reach = if f.per_unit_us_p99 > 0.0 && f.limit.is_finite() {
                format!("At this rate the tick p99 reaches 10 ms at about {:.0} worms", f.limit)
            } else {
                "The tick p99 does not grow with the worms here".to_string()
            };
            let _ = writeln!(
                o,
                "\nCost per worm: {:.0} µs of tick at p50, {:.0} µs at p99. {reach} (a projection: `MAX_WORMS` stays 32).",
                f.per_unit_us_p50, f.per_unit_us_p99
            );
        } else {
            let _ = writeln!(
                o,
                "\nTick p99 grows {:.4} µs per unit of {} (least squares).",
                f.per_unit_us_p99, s.axis
            );
        }
    }
    let _ = writeln!(o);
}

/// Megapixels of a size.
fn mpx(s: (u32, u32)) -> f64 {
    f64::from(s.0) * f64::from(s.1) / 1e6
}

/// Busy p50 (ms) against the window's megapixels, least squares: the
/// slope (ms per million pixels), when it grows with the pixels (the
/// largest window's busy p50 at least 1.5 times the smallest's and at
/// least 0.25 ms more per million pixels); `None` when it stays flat.
fn busy_growth(rows: &[Windowed]) -> Option<f64> {
    if rows.len() < 2 {
        return None;
    }
    let pts: Vec<(f64, f64)> = rows.iter().map(|w| (mpx(w.size), w.busy_ms.p50)).collect();
    let n = pts.len() as f64;
    let (sx, sy) = pts.iter().fold((0.0, 0.0), |(a, b), &(x, y)| (a + x, b + y));
    let sxx: f64 = pts.iter().map(|&(x, _)| x * x).sum();
    let sxy: f64 = pts.iter().map(|&(x, y)| x * y).sum();
    let d = n * sxx - sx * sx;
    if d.abs() < 1e-12 {
        return None;
    }
    let slope = (n * sxy - sx * sy) / d;
    let by_px = |a: &&Windowed, b: &&Windowed| mpx(a.size).total_cmp(&mpx(b.size));
    let (lo, hi) = (rows.iter().min_by(by_px)?, rows.iter().max_by(by_px)?);
    (slope >= 0.25 && hi.busy_ms.p50 >= 1.5 * lo.busy_ms.p50).then_some(slope)
}

/// Where a budget is missed: "every size", "none" or the sizes.
fn missed(rows: &[Windowed], budget: f64) -> String {
    let m: Vec<String> = rows.iter().filter(|w| w.frame_ms.p99 > budget).map(|w| size(w.size)).collect();
    match m.len() {
        0 => "holds at every size".into(),
        n if n == rows.len() => "missed at every size".into(),
        _ => format!("missed at {}", m.join(", ")),
    }
}

/// Whether every windowed row holds both frame budgets.
fn holds_everywhere(rows: &[Windowed], b: &Budgets) -> bool {
    !rows.is_empty() && rows.iter().all(|w| w.frame_ms.p99 <= b.frame_144_ms)
}

/// A path's windowed runs. The text follows what was measured (the present
/// mode, the busy time against the pixels, the budgets), not the path's
/// name; `holding` are the other paths whose windowed runs hold both
/// budgets at every size.
fn windowed(o: &mut String, g: &GpuPath, b: &Budgets, holding: &[&str]) {
    let rows = &g.windowed;
    let _ = writeln!(
        o,
        "\nWindowed, asked for uncapped (`olr-game --frame-log --uncapped`, HUD included). Frame is frame to frame \
         (what the player sees); busy is a frame's CPU time without waiting to present.\n"
    );
    let fifo: Vec<String> =
        rows.iter().filter(|w| present_mode(&w.gpu) == "Fifo").map(|w| size(w.size)).collect();
    if !fifo.is_empty() {
        let which = if fifo.len() == rows.len() { "Every run".to_string() } else { fifo.join(", ") };
        let _ = writeln!(
            o,
            "{which} presented with Fifo: the surface offered no uncapped mode, so a frame also waits for the \
             display's refresh and the frame rate is at most the refresh rate.\n"
        );
    }
    if let Some(slope) = busy_growth(rows) {
        let by_px = |a: &&Windowed, b: &&Windowed| mpx(a.size).total_cmp(&mpx(b.size));
        let (lo, hi) = (rows.iter().min_by(by_px), rows.iter().max_by(by_px));
        if let (Some(lo), Some(hi)) = (lo, hi) {
            let off: Vec<f64> = g.sizes.iter().map(|s| s.frame_ms.p50).collect();
            let offscreen = if off.is_empty() {
                String::new()
            } else {
                let (a, z) = off.iter().fold((f64::MAX, f64::MIN), |(a, z), &x| (a.min(x), z.max(x)));
                format!(
                    ", while the offscreen frame's p50 stays {a:.2}–{z:.2} ms at the same sizes (table above)"
                )
            };
            let _ = writeln!(
                o,
                "The busy time grows with the window's pixels: {:.1} ms p50 at {} ({:.1} Mpx) to {:.1} ms at {} \
                 ({:.1} Mpx), about {slope:.1} ms per million pixels (least squares over the p50s){offscreen}. \
                 Presenting costs this path CPU time per pixel, so its windowed frame rate falls with the \
                 resolution: this is the cost of playing on this path, not of drawing the game.\n",
                lo.busy_ms.p50,
                size(lo.size),
                mpx(lo.size),
                hi.busy_ms.p50,
                size(hi.size),
                mpx(hi.size),
            );
        }
    }
    let _ = writeln!(
        o,
        "Against the budgets (frame p99): 144 Hz ({:.1} ms) {}; 60 Hz ({:.1} ms) {}.",
        b.frame_144_ms,
        missed(rows, b.frame_144_ms),
        b.frame_60_ms,
        missed(rows, b.frame_60_ms)
    );
    if !holds_everywhere(rows, b) && !holding.is_empty() {
        let native = if holding.iter().all(|k| k.starts_with("windows-")) {
            " (the native Windows build)"
        } else {
            ""
        };
        let _ = writeln!(
            o,
            "On {}{native} the windowed runs hold both budgets at every size (their tables): at high \
             resolutions, play there rather than on this path.",
            holding.join(" and ")
        );
    }
    let _ = writeln!(
        o,
        "\n| asked | got | present mode | frames | FPS | frame p50 / p99 (ms) | busy p50 / p99 (ms) | holds 144 / 60 Hz |\n\
         |---|---|---|---|---|---|---|---|"
    );
    let yes = |ok: bool| if ok { "yes" } else { "no" };
    for w in rows {
        let got = if w.reached {
            size(w.size)
        } else {
            format!("{} (monitor smaller: see offscreen)", size(w.size))
        };
        let _ = writeln!(
            o,
            "| {} | {got} | {} | {} | {:.0} | {:.2} / {:.2} | {:.2} / {:.2} | {} / {} |",
            size(w.requested),
            present_mode(&w.gpu),
            w.frames,
            w.fps,
            w.frame_ms.p50,
            w.frame_ms.p99,
            w.busy_ms.p50,
            w.busy_ms.p99,
            yes(w.frame_ms.p99 <= b.frame_144_ms),
            yes(w.frame_ms.p99 <= b.frame_60_ms)
        );
    }
}

fn gpu(o: &mut String, key: &str, g: &GpuPath, b: &Budgets, holding: &[&str]) {
    let _ = writeln!(o, "### {key}: {} ({}, {})\n", g.adapter, g.backend, g.device_type);
    let _ = writeln!(
        o,
        "GPU times: {}.",
        if g.timestamps {
            "timestamp queries"
        } else {
            "none (no timestamp queries): CPU-side frame times only"
        }
    );
    if !g.notes.is_empty() {
        let _ = writeln!(o);
    }
    for n in &g.notes {
        let _ = writeln!(o, "- {n}");
    }
    let _ = writeln!(
        o,
        "\nThe heavy replay (MW 1.0, 32 chaos bots, a 20 000 pool) offscreen, a tick and a frame each (the \
         replay's recorded inputs: its tick has no bots' thinking):\n\n\
         | size | tick (no bots) + frame p50 / p99 (ms) | frame p99, CPU then GPU (ms) | GPU p99 (ms) |\n|---|---|---|---|"
    );
    for s in &g.sizes {
        let gp = s.gpu_ms.map_or("—".to_string(), |x| format!("{:.2}", x.p99));
        let _ = writeln!(
            o,
            "| {} | {:.2} / {:.2} | {:.2} | {gp} |",
            size(s.size),
            s.total_ms.p50,
            s.total_ms.p99,
            s.frame_ms.p99
        );
    }
    let _ = writeln!(
        o,
        "\nShader headroom, GPU side (a full-screen blur pass, `olr_render::Probe`): the GPU time one pass \
         costs, and how many passes fit beside the frame's GPU p99 (its frame p99 without timestamps) within \
         the {:.1} ms (144 Hz) and {:.1} ms (60 Hz) budgets. This is GPU-side headroom, not the whole frame: \
         the tick and the draw list run on the CPU beside it.\n\n\
         | size | GPU ms per pass | passes that fit at 144 Hz | at 60 Hz |\n|---|---|---|---|",
        b.frame_144_ms, b.frame_60_ms
    );
    for s in &g.sizes {
        let _ =
            writeln!(o, "| {} | {:.3} | {} | {} |", size(s.size), s.ms_per_pass, s.passes_144, s.passes_60);
    }
    if !g.windowed.is_empty() {
        let others: Vec<&str> = holding.iter().copied().filter(|k| *k != key).collect();
        windowed(o, g, b, &others);
    }
    if !g.levels.is_empty() {
        let _ = writeln!(
            o,
            "\nThe sweeps' levels drawn offscreen (their replays: the tick has no bots' thinking; the sweeps' \
             tables add it):\n\n| axis | value | size | tick (no bots) + frame p99 (ms) | GPU p99 (ms) |\n|---|---|---|---|---|"
        );
        for l in &g.levels {
            let gp = l.gpu_ms.map_or("—".to_string(), |x| format!("{:.2}", x.p99));
            let _ =
                writeln!(o, "| {} | {} | {} | {:.2} | {gp} |", l.axis, l.value, size(l.size), l.total_ms.p99);
        }
    }
    let mut dropped: Vec<String> = g
        .sizes
        .iter()
        .filter(|s| s.gpu_dropped + s.probe_dropped > 0)
        .map(|s| {
            format!(
                "{}: {}, {}",
                size(s.size),
                count(s.gpu_dropped, "frame"),
                count(s.probe_dropped, "probe frame")
            )
        })
        .collect();
    dropped.extend(
        g.levels
            .iter()
            .filter(|l| l.gpu_dropped > 0)
            .map(|l| format!("{} {}: {}", l.axis, l.value, count(l.gpu_dropped, "frame"))),
    );
    if !dropped.is_empty() {
        let _ = writeln!(
            o,
            "\nTimestamps without a real span (unwritten or backwards, a driver's fault), over every run; those \
             frames are left out of the GPU times, not counted as 0 ms:\n"
        );
        for d in &dropped {
            let _ = writeln!(o, "- {d}");
        }
    }
    let _ = writeln!(o);
}

/// The rollback axis (plan 9, spec §3.3): the cost of restoring and
/// re-simulating, against the budgets.
fn rollback(o: &mut String, r: &Rollback) {
    let _ = writeln!(o, "{}\n", r.what);
    let _ = writeln!(
        o,
        "| point | worms | depth | restore p50 / p99 (µs) | re-simulation p50 / p99 (µs) | total p99 (µs) | \
         snapshot (B) | journal, at most (B) | budget |\n|---|---|---|---|---|---|---|---|---|"
    );
    for p in &r.points {
        let _ = writeln!(
            o,
            "| {} | {} | {} | {:.0} / {:.0} | {:.0} / {:.0} | {:.0} | {} | {} | {} |",
            p.label,
            p.worms,
            p.depth,
            p.restore_us_p50,
            p.restore_us_p99,
            p.resim_us_p50,
            p.resim_us_p99,
            p.total_us_p99,
            p.snapshot_bytes,
            p.journal_bytes_max,
            p.verdict()
        );
    }
    let broken: Vec<&str> = r.points.iter().filter(|p| p.over_budget()).map(|p| p.label.as_str()).collect();
    if broken.is_empty() {
        let _ = writeln!(o, "\nEvery budgeted point holds.");
    } else {
        let _ = writeln!(o, "\nBreaking: {}.", broken.join(", "));
    }
    let _ = writeln!(o);
}

fn olx(o: &mut String, c: &OlxComparison) {
    let _ = writeln!(
        o,
        "Image `{}`, built {}. OLX runs in a Docker container on WSL's own kernel (no extra virtualisation, the \
         same clock). Its timed span is the gus logic and the client's and the server's frame, without the \
         trace writing; shot-list delivery and message parsing run in OLX's `betweenFrames()`, outside that \
         span, while olr-sim does the equivalent inside `World::tick`, so the comparison slightly flatters OLX. \
         olr-sim's span (`olr-cli scenario --time`) also holds a scenario's bonus placement (none in these \
         scenarios). OLX draws on the CPU at 640 × 480 and is not timed here (not comparable like for like).\n",
        c.image, c.build
    );
    let full: Vec<&str> = c
        .rows
        .iter()
        .filter(|r| r.projectiles_peak >= OLX_MAX_PROJECTILES)
        .map(|r| r.name.as_str())
        .collect();
    let mid: Vec<String> = c
        .rows
        .iter()
        .filter(|r| r.name.starts_with("heavy-") && r.projectiles_peak < OLX_MAX_PROJECTILES)
        .map(|r| format!("{} is a mid-load point ({} projectiles at its peak)", r.name, r.projectiles_peak))
        .collect();
    let mut load = match full.as_slice() {
        [] => format!("No scenario here fills OLX's {OLX_MAX_PROJECTILES}-projectile pool"),
        [one] => format!("{one} fills OLX's {OLX_MAX_PROJECTILES}-projectile pool (`MAX_PROJECTILES`)"),
        many => format!(
            "{} fill OLX's {OLX_MAX_PROJECTILES}-projectile pool (`MAX_PROJECTILES`)",
            many.join(", ")
        ),
    };
    for m in &mid {
        let _ = write!(load, "; {m}");
    }
    let _ = writeln!(
        o,
        "Load: {load}. The others are the busiest oracle fixtures. Same work: the same projectiles and `rand()` \
         calls every frame in both.\n"
    );
    let _ = writeln!(
        o,
        "| scenario | frames | peak projectiles | OLX p50 / p99 (µs) | olr-sim p50 / p99 (µs) | olr-sim / OLX (p50) | same work | matches OLX |\n\
         |---|---|---|---|---|---|---|---|"
    );
    for r in &c.rows {
        let _ = writeln!(
            o,
            "| {} | {} | {} | {:.1} / {:.1} | {:.1} / {:.1} | {:.2} | {} | {} |",
            r.name,
            r.frames,
            r.projectiles_peak,
            r.olx_us.p50,
            r.olx_us.p99,
            r.olr_us.p50,
            r.olr_us.p99,
            r.ratio_p50,
            if r.same_work { "yes" } else { "no" },
            r.matches_olx.map_or("— (timing trace)", |m| if m { "yes" } else { "no" })
        );
    }
    if !c.notes.is_empty() {
        let _ = writeln!(o, "\nNot timed:\n");
        for n in &c.notes {
            let _ = writeln!(o, "- {n}");
        }
    }
}

/// The report of `env` with `summary` (kept as given).
pub fn render(env: &Envelope, summary: &str) -> String {
    let b = env.budgets;
    let mut o = String::new();
    let _ = writeln!(o, "# Performance envelope\n");
    let _ = writeln!(
        o,
        "Made by `cargo xtask envelope --report` from the results measured on {} ({} run{} per point{}). \
         Everything but the summary is regenerated; edit only the summary.\n",
        env.date,
        env.runs,
        if env.runs == 1 { "" } else { "s" },
        if env.quick { ", `--quick`" } else { "" }
    );
    if !env.kept.is_empty() {
        let _ = writeln!(
            o,
            "Kept from an earlier bless (not measured in the last one): {}.\n",
            env.kept.join(", ")
        );
    }
    let _ = writeln!(o, "## Headroom summary\n\n{START}\n{}\n{END}\n", summary.trim_matches('\n'));
    let _ = writeln!(
        o,
        "## Method\n\n\
         - Budgets: tick p99 ≤ {} ms (OLX's 100 Hz frame), frame p99 ≤ {:.1} ms (144 Hz) and ≤ {:.1} ms (60 Hz), \
         peak memory below {:.0} MiB. A point holds when all of them do; the breaking point is the first value that \
         does not, with the cause (tick, frame, memory, or a cap).\n\
         - Sweeps: `olr-cli stress`, one process a point, medians of the runs; the reference is Classic on Castle \
         Strike, 16 bots of mixed difficulty (chaos bots; Deathmatch bots on the map and art sweeps, so route \
         finding counts), OLX's pool, {} s, seed 1, `LoadingTime` 0, bonuses every 5 s. The pool sweep plays \
         MW 1.0. Map and art levels are generated in memory (`gen-<w>x<h>-a<scale>-s1`).\n\
         - Frames: `olr-cli gpu-bench` offscreen on the hardware adapter (timestamps when the adapter has them), and \
         `olr-game --frame-log --uncapped` windowed; on WSL (GL over Mesa's d3d12, presented through WSLg) and \
         native Windows (DX12, Vulkan) built with mingw-w64 and run through interop.\n\
         - Rollback: `olr-cli rollback-cost`, a Classic match of 16 chaos bots (500 ticks of warm-up, then a \
         sample every 10 ticks of play): save, play `depth` ticks, then time the restore and the re-simulation \
         of those ticks with their recorded keys (the bots' thinking is not timed: online, bots think on the host \
         only). Budgets: restore plus 12 ticks p99 ≤ 2 ms at the reference (Castle Strike) and ≤ 4 ms at \
         4096 × 4096; depth 28 (`MAX_PREDICTION`) is information.\n\
         - OLX: the oracle's patched OLX (Docker; a Release build, -O3) timing each simulated frame to a file of \
         its own; olr-sim timed on the same scenarios, the same work checked frame by frame.\n",
        b.tick_ms, b.frame_144_ms, b.frame_60_ms, b.memory_mib, env.seconds
    );
    let _ = writeln!(o, "## The machine\n\n- CPU: {} ({} logical CPUs)", env.host.cpu, env.host.cpus);
    for (k, g) in &env.gpu {
        let _ = writeln!(o, "- {k}: {} ({}, {})", g.adapter, g.backend, g.device_type);
    }
    let _ = writeln!(o, "\n## Sweeps\n");
    for s in env.sweeps.values() {
        sweep(&mut o, s, &b);
    }
    let _ = writeln!(o, "## Rollback\n");
    match &env.rollback {
        Some(r) => rollback(&mut o, r),
        None => {
            let _ = writeln!(o, "Not measured in these results.\n");
        }
    }
    let _ = writeln!(o, "## GPU\n");
    let holding: Vec<&str> =
        env.gpu.iter().filter(|(_, g)| holds_everywhere(&g.windowed, &b)).map(|(k, _)| k.as_str()).collect();
    for (k, g) in &env.gpu {
        gpu(&mut o, k, g, &b, &holding);
    }
    let _ = writeln!(o, "## OLX\n");
    match &env.olx {
        Some(c) => olx(&mut o, c),
        None => {
            let _ = writeln!(o, "Not measured in these results.");
        }
    }
    let _ = writeln!(
        o,
        "\n### Limits\n\n| | OLX (`d1ad953f`) | rust-liero |\n|---|---|---|\n\
         | Worms | 32 (`MAX_WORMS`, `include/Consts.h:34`) | 32 (`olr_sim::MAX_WORMS`) |\n\
         | Projectiles | 3000 (`MAX_PROJECTILES`, `include/CProjectile.h:36`) | 3000 by default, up to 65 536 (`MatchSetup::MAX_POOL_CAPACITY`) |\n\
         | Map side | 4096 (12-bit worm positions) | 4096 for imported levels (`olr-import-lxl`) |\n\
         | Effects | 1024 (`MAX_ENTITIES`, `include/Entity.h:30`) | 16 384 in play, 1024 of them kept for deaths (`olr_fx::PLAY_ENTITIES`, `DEATH_RESERVE`); OLX's 1024 with `olx_quirks` (`olr_fx::MAX_ENTITIES`); `Fx::with_capacity` in benches |\n\
         | Colour image | — | 256 MiB (`MAX_COLOR_MAP_BYTES`), art scale at most 8 |"
    );
    o
}

/// The summary a new report keeps from `existing` (the report on disk):
/// the placeholder when there is none yet; an error when the report has
/// lost its markers, rather than dropping a hand-written summary.
fn summary_to_keep(existing: Option<&str>) -> anyhow::Result<String> {
    match existing {
        None => Ok(NO_SUMMARY.to_string()),
        Some(text) => summary_of(text).ok_or_else(|| {
            anyhow::anyhow!(
                "{REPORT} has no `{START}` ... `{END}` markers: put them back around the summary \
                 (or remove the file) before regenerating, so the hand-written summary is not lost"
            )
        }),
    }
}

/// Writes the report of this host's results file (or `from`), keeping the summary.
pub fn write(from: Option<&Path>) -> anyhow::Result<PathBuf> {
    let file = match from {
        Some(p) => p.to_path_buf(),
        None => root().join(results::path_for(&crate::bench::Host::here())),
    };
    let env = results::load(&file).map_err(anyhow::Error::msg)?.ok_or_else(|| {
        anyhow::anyhow!(
            "no results at {} (`cargo xtask envelope --bless --reason \"<why>\"` first)",
            file.display()
        )
    })?;
    let out = root().join(REPORT);
    let summary = summary_to_keep(std::fs::read_to_string(&out).ok().as_deref())?;
    if let Some(dir) = out.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&out, render(&env, &summary))?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::envelope::results::{
        Breaking, Fit, GpuLevel, GpuSize, OlxComparison, OlxRow, Point, Spread, Sweep, Windowed,
    };

    fn env() -> Envelope {
        let mut e = Envelope { date: "2026-10-11".into(), runs: 3, ..Envelope::default() };
        let p = |v: &str, n: f64, p99: f64| Point {
            value: v.into(),
            n,
            runs: 3,
            tick_us: Spread { p50: p99 / 2.0, p99, max: p99 },
            ..Point::default()
        };
        e.sweeps.insert(
            "worms".into(),
            Sweep {
                axis: "worms".into(),
                points: vec![p("16", 16.0, 2000.0), p("32", 32.0, 12_000.0)],
                breaking_144: Some(Breaking {
                    value: "32".into(),
                    causes: vec!["tick p99 12.00 ms > 10 ms".into()],
                }),
                fit: Some(Fit {
                    per_unit_us_p50: 150.0,
                    per_unit_us_p99: 300.0,
                    intercept_us_p99: 100.0,
                    limit: 33.0,
                }),
                ..Sweep::default()
            },
        );
        e.gpu.insert(
            "wsl-gl".into(),
            GpuPath {
                adapter: "RTX".into(),
                timestamps: true,
                sizes: vec![GpuSize {
                    size: (3840, 2160),
                    ms_per_pass: 0.25,
                    passes_144: 24,
                    passes_60: 62,
                    ..GpuSize::default()
                }],
                ..GpuPath::default()
            },
        );
        e
    }

    /// Review Focus 5.
    #[test]
    fn regenerating_keeps_the_hand_written_summary() {
        let summary = "Shaders: about 24 passes at 4K.\n\nMaps: 4096² holds.";
        let doc = render(&env(), summary);
        assert_eq!(summary_of(&doc).as_deref(), Some(summary));
        assert_eq!(render(&env(), &summary_of(&doc).unwrap()), doc, "regenerating is stable");
        assert_eq!(summary_of("no markers"), None);
    }

    /// A report whose markers were lost is refused, never overwritten with
    /// the placeholder (the hand-written summary would be gone).
    #[test]
    fn a_report_without_markers_is_not_overwritten() {
        assert_eq!(summary_to_keep(None).unwrap(), NO_SUMMARY, "no report yet");
        let doc = render(&env(), "Shaders: plenty.");
        assert_eq!(summary_to_keep(Some(&doc)).unwrap(), "Shaders: plenty.");
        let lost = doc.replace(END, "");
        let e = summary_to_keep(Some(&lost)).unwrap_err().to_string();
        assert!(e.contains("no `<!-- summary:start -->`") && e.contains("not lost"), "{e}");
    }

    #[test]
    fn the_report_names_breaking_points_projections_and_probe_fits() {
        let doc = render(&env(), "s");
        for want in [
            "Breaking point (144 Hz): **32**",
            "tick p99 12.00 ms > 10 ms",
            "reaches 10 ms at about 33 worms",
            "| 3840×2160 | 0.250 | 24 | 62 |",
            "RTX",
            "| Projectiles | 3000",
        ] {
            assert!(doc.contains(want), "missing {want:?} in:\n{doc}");
        }
    }

    /// A sweep's frame column and its 144 Hz verdict use one figure: the
    /// level's tick and frame plus the bots' thinking.
    #[test]
    fn the_sweep_frame_includes_the_bots_thinking() {
        use crate::envelope::results::FrameCheck;
        let mut e = env();
        let w = e.sweeps.get_mut("worms").unwrap();
        w.points[0].think_us = Spread { p50: 500.0, p99: 1500.0, max: 1500.0 };
        w.points[0].frame = Some(FrameCheck {
            path: "wsl-gl".into(),
            size: (2560, 1440),
            total_ms: Spread { p50: 3.0, p99: 6.0, max: 6.0 },
            gpu_ms: None,
        });
        let doc = render(&e, "s");
        assert!(doc.contains("| 7.50 | frame p99 7.50 ms (6.00 drawn + 1.50 bots) > 6.9 ms |"), "{doc}");
        assert!(doc.contains("frame with bots p99 (ms)") && doc.contains("no bots' thinking"), "{doc}");
    }

    /// The shader headroom is the GPU side of a frame, not the whole frame.
    #[test]
    fn probe_passes_are_gpu_side_headroom() {
        let doc = render(&env(), "s");
        assert!(doc.contains("GPU-side headroom"), "{doc}");
        assert!(doc.contains("not the whole frame"), "{doc}");
    }

    fn windowed(gpu: &str, size: (u32, u32), frame_p99: f64, busy_p50: f64) -> Windowed {
        Windowed {
            requested: size,
            size,
            reached: true,
            gpu: gpu.into(),
            frames: 179,
            fps: 35.7,
            frame_ms: Spread { p50: frame_p99 * 0.8, p99: frame_p99, max: frame_p99 * 1.1 },
            busy_ms: Spread { p50: busy_p50, p99: busy_p50 * 1.3, max: busy_p50 * 1.4 },
        }
    }

    const SIZES: [(u32, u32); 4] = [(1920, 1080), (2560, 1440), (3440, 1440), (3840, 2160)];

    /// The measured WSL rows (2026-10-12): busy time grows with the pixels.
    fn presented_per_pixel() -> Vec<Windowed> {
        let gpu = "D3D12 (NVIDIA GeForce RTX 5070 Ti) (Gl, Other), Fifo, Bgra8Unorm";
        SIZES
            .iter()
            .zip([(11.49, 5.70), (18.59, 9.56), (20.42, 12.65), (32.82, 20.34)])
            .map(|(&z, (f, b))| windowed(gpu, z, f, b))
            .collect()
    }

    fn native() -> GpuPath {
        let gpu = "NVIDIA GeForce RTX 5070 Ti (Dx12, DiscreteGpu), Immediate, Bgra8UnormSrgb";
        GpuPath {
            adapter: "NVIDIA GeForce RTX 5070 Ti".into(),
            windowed: SIZES.iter().map(|&z| windowed(gpu, z, 2.4, 0.6)).collect(),
            ..GpuPath::default()
        }
    }

    /// The windowed text follows the measurements (present mode, busy
    /// time against the pixels, the budgets), never the path's name.
    #[test]
    fn windowed_runs_are_explained_by_what_was_measured() {
        let mut e = env();
        e.gpu.get_mut("wsl-gl").unwrap().windowed = presented_per_pixel();
        e.gpu.insert("windows-dx12".into(), native());
        let doc = render(&e, "s");
        let wsl = &doc[doc.find("### wsl-gl").unwrap()..];
        for want in [
            "Every run presented with Fifo",
            "5.7 ms p50 at 1920×1080 (2.1 Mpx) to 20.3 ms at 3840×2160 (8.3 Mpx), about 2.4 ms per million pixels",
            "Presenting costs this path CPU time per pixel",
            "144 Hz (6.9 ms) missed at every size; 60 Hz (16.7 ms) missed at 2560×1440, 3440×1440, 3840×2160.",
            "On windows-dx12 (the native Windows build) the windowed runs hold both budgets at every size",
            "| 1920×1080 | 1920×1080 | Fifo | 179 | 36 | 9.19 / 11.49 | 5.70 / 7.41 | no / yes |",
        ] {
            assert!(wsl.contains(want), "missing {want:?} in:\n{wsl}");
        }
        assert!(!wsl.contains("WSLg"), "not keyed on the name: {wsl}");
        let dx12 = &doc[doc.find("### windows-dx12").unwrap()..doc.find("### wsl-gl").unwrap()];
        assert!(
            dx12.contains("144 Hz (6.9 ms) holds at every size; 60 Hz (16.7 ms) holds at every size."),
            "{dx12}"
        );
        for not in ["Fifo", "per million pixels", "play there"] {
            assert!(!dx12.contains(not), "{not:?} in {dx12}");
        }
        // The same rows under another name read the same.
        let mut renamed = env();
        renamed.gpu.clear();
        renamed
            .gpu
            .insert("windows-gl".into(), GpuPath { windowed: presented_per_pixel(), ..GpuPath::default() });
        let doc = render(&renamed, "s");
        assert!(
            doc.contains("about 2.4 ms per million pixels") && doc.contains("Every run presented with Fifo")
        );
    }

    /// Timestamps without a real span are left out of the GPU times, and said so.
    #[test]
    fn dropped_gpu_spans_are_reported() {
        let mut e = env();
        assert!(!render(&e, "s").contains("without a real span"));
        let wsl = e.gpu.get_mut("wsl-gl").unwrap();
        wsl.sizes[0].gpu_dropped = 3;
        wsl.sizes[0].probe_dropped = 2;
        wsl.levels.push(GpuLevel {
            axis: "map".into(),
            value: "4096x4096".into(),
            gpu_dropped: 1,
            ..GpuLevel::default()
        });
        let doc = render(&e, "s");
        for want in ["3840×2160: 3 frames, 2 probe frames", "map 4096x4096: 1 frame", "without a real span"]
        {
            assert!(doc.contains(want), "missing {want:?} in:\n{doc}");
        }
    }

    fn row(name: &str, peak: u32) -> OlxRow {
        OlxRow {
            name: name.into(),
            frames: 400,
            projectiles_peak: peak,
            same_work: true,
            ..OlxRow::default()
        }
    }

    /// The OLX build, the scenarios' load as measured, what the spans leave out and the notes.
    #[test]
    fn the_rollback_table_flags_a_broken_budget() {
        use super::super::results::{Rollback, RollbackPoint};
        let point = |label: &str, depth: u32, total: f64, budget: Option<f64>| RollbackPoint {
            label: label.into(),
            worms: 16,
            depth,
            runs: 3,
            total_us_p99: total,
            budget_us: budget,
            ..RollbackPoint::default()
        };
        let mut e = env();
        e.rollback = Some(Rollback {
            what: "Rollback what.".into(),
            points: vec![
                point("castlestrike-12", 12, 1500.0, Some(2000.0)),
                point("castlestrike-28", 28, 3000.0, None),
                point("4096-12", 12, 4500.0, Some(4000.0)),
            ],
            noise: 0.05,
        });
        let doc = render(&e, "s");
        let rb = &doc[doc.find("## Rollback").unwrap()..doc.find("## GPU").unwrap()];
        for want in [
            "| castlestrike-12 | 16 | 12 |",
            "holds (2000 µs budget)",
            "no budget",
            "BREAKS the 4000 µs budget (total p99 4500 µs)",
            "Breaking: 4096-12.",
        ] {
            assert!(rb.contains(want), "missing {want:?} in:\n{rb}");
        }
    }

    #[test]
    fn the_olx_comparison_says_what_it_measured() {
        let mut e = env();
        e.olx = Some(OlxComparison {
            image: "rust-liero/olx-oracle:d1ad953f".into(),
            build: "Release, -O3 (CMakeOlxCommon.cmake:43-47, :369)".into(),
            rows: vec![row("two-worms-fire", 4), row("heavy-spikeballs", 3000), row("heavy-napalm", 829)],
            notes: vec!["multi-shot: OLX timed 3 frames of 200".into()],
        });
        let doc = render(&e, "s");
        let olx = &doc[doc.find("## OLX").unwrap()..];
        for want in [
            "Release, -O3",
            "heavy-spikeballs fills OLX's 3000-projectile pool",
            "heavy-napalm is a mid-load point (829 projectiles at its peak)",
            "betweenFrames()",
            "slightly flatters OLX",
            "- multi-shot: OLX timed 3 frames of 200",
        ] {
            assert!(olx.contains(want), "missing {want:?} in:\n{olx}");
        }
        assert!(!olx.contains("heavy-napalm fills"), "{olx}");
    }
}
