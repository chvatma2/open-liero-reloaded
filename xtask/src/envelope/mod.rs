//! `cargo xtask envelope` (plan 8; spec
//! `docs/superpowers/specs/2026-10-11-performance-envelope-design.md`):
//! how far each axis goes on this machine before the tick, frame or
//! memory budget breaks, the GPU on WSL and native Windows with the shader
//! probe, and OLX's frame times. Every run writes `target/envelope/latest.ron`
//! and compares with `fixtures/bench/envelope/<host>.ron` (beyond each
//! axis's noise is flagged, never failed); `--bless --reason` writes that
//! file and logs it. Not part of `full`.

pub mod gpu;
pub mod olx;
pub mod report;
pub mod results;
pub mod sweeps;

use std::path::Path;

use crate::bench::Host;
use crate::root;
use results::{Budgets, Envelope, GpuPath, OlxComparison, Sweep};

/// A part of the envelope.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, clap::ValueEnum)]
pub enum Axis {
    Worms,
    Pool,
    Fx,
    Map,
    Art,
    Gpu,
    Olx,
}

impl Axis {
    pub const ALL: [Axis; 7] =
        [Axis::Worms, Axis::Pool, Axis::Fx, Axis::Map, Axis::Art, Axis::Gpu, Axis::Olx];

    pub fn name(self) -> &'static str {
        match self {
            Axis::Worms => "worms",
            Axis::Pool => "pool",
            Axis::Fx => "fx",
            Axis::Map => "map",
            Axis::Art => "art",
            Axis::Gpu => "gpu",
            Axis::Olx => "olx",
        }
    }

    pub fn is_sweep(self) -> bool {
        !matches!(self, Axis::Gpu | Axis::Olx)
    }
}

/// Which GPU paths run.
#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum GpuPaths {
    Wsl,
    Windows,
    Both,
    None,
}

/// What a run measures (spec §4, §5; `--quick` one run per point, fewer points).
#[derive(Clone, Debug)]
pub struct Plan {
    pub quick: bool,
    pub runs: usize,
    pub seconds: f32,
    pub worms: Vec<u32>,
    pub pools: Vec<u32>,
    pub fx: Vec<u32>,
    pub maps: Vec<(u32, u32)>,
    pub arts: Vec<u32>,
    pub heavy_seconds: f32,
    pub sizes: Vec<(u32, u32)>,
    pub start_tick: u32,
    pub gpu_frames: u32,
    pub probe_passes: Vec<u32>,
    pub probe_frames: u32,
    pub level_start: u32,
    pub level_frames: u32,
    pub windowed_seconds: f64,
    pub olx_timed: Vec<&'static str>,
    pub olx_heavy: Vec<&'static str>,
}

impl Plan {
    pub fn new(quick: bool) -> Plan {
        use crate::olx_timing::{HEAVY, TIMED};
        if quick {
            return Plan {
                quick,
                runs: 1,
                seconds: 5.0,
                worms: vec![2, 16, 32],
                pools: vec![3000, 65_536],
                fx: vec![1024, 65_536],
                maps: vec![(1024, 1024), (4096, 4096)],
                arts: vec![1, 8],
                heavy_seconds: 15.0,
                sizes: vec![(1920, 1080), (3840, 2160)],
                start_tick: 500,
                gpu_frames: 200,
                probe_passes: vec![0, 4, 16],
                probe_frames: 40,
                level_start: 200,
                level_frames: 100,
                windowed_seconds: 8.0,
                olx_timed: TIMED[..2].to_vec(),
                olx_heavy: HEAVY[..1].to_vec(),
            };
        }
        Plan {
            quick,
            runs: 3,
            seconds: 20.0,
            worms: vec![2, 4, 8, 16, 24, 32],
            pools: vec![3000, 10_000, 20_000, 50_000, 65_536],
            fx: vec![1024, 4096, 16_384, 65_536],
            maps: vec![(1024, 1024), (2048, 2048), (4096, 2048), (4096, 4096)],
            arts: vec![1, 2, 4, 8],
            heavy_seconds: 40.0,
            sizes: vec![(1920, 1080), (2560, 1440), (3440, 1440), (3840, 2160)],
            start_tick: 1000,
            gpu_frames: 600,
            probe_passes: vec![0, 1, 2, 4, 8, 16, 32],
            probe_frames: 120,
            level_start: 500,
            level_frames: 300,
            windowed_seconds: 15.0,
            olx_timed: TIMED.to_vec(),
            olx_heavy: HEAVY.to_vec(),
        }
    }
}

pub struct Opts {
    pub quick: bool,
    pub only: Vec<Axis>,
    pub gpu: GpuPaths,
    pub bless: Option<String>,
    /// Write `docs/perf/envelope.md` from the results and stop (`report.rs`).
    pub report: bool,
    /// The results file the report is made from (else this host's).
    pub from: Option<std::path::PathBuf>,
}

/// What the parts beyond the CPU sweeps need from this machine (ruling
/// PF5): each check says why its part cannot run here (`None`: it can).
/// The command prints that as a `skip` notice and goes on without the
/// part ([`steps`]); a test stands in for the machine.
pub struct Prereqs {
    /// The Windows GPU paths: interop and Windows PowerShell, the rustup
    /// target, mingw-w64 (`winbuild::missing`).
    pub windows: fn() -> Option<String>,
    /// The OLX comparison: Docker installed and its daemon running
    /// (`olx_timing::docker_missing`).
    pub docker: fn() -> Option<String>,
    /// The WSL path's windowed runs: a display.
    pub display: fn() -> Option<String>,
}

impl Prereqs {
    /// This machine's.
    pub fn here() -> Prereqs {
        Prereqs {
            windows: crate::winbuild::missing,
            docker: crate::olx_timing::docker_missing,
            display: display_missing,
        }
    }
}

/// Why no window can open here (WSLg sets `DISPLAY` and `WAYLAND_DISPLAY`).
fn display_missing() -> Option<String> {
    let var = |k: &str| std::env::var(k).ok();
    (!gpu::has_display(var("DISPLAY").as_deref(), var("WAYLAND_DISPLAY").as_deref()))
        .then(|| "no display (DISPLAY and WAYLAND_DISPLAY are unset)".to_string())
}

/// One step of a run, in order.
#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    Sweep(Axis),
    /// The WSL GPU path; windowed runs too when a window can open.
    GpuWsl {
        windowed: bool,
    },
    /// DX12 and Vulkan through Windows interop.
    GpuWindows,
    Olx,
    /// A part that cannot run here, and why: a notice, never a failure.
    Skip(String),
}

/// What `axes` and `gpu` ask for on a machine with `pre`.
pub fn steps(axes: &[Axis], gpu: GpuPaths, pre: &Prereqs) -> Vec<Step> {
    let mut v: Vec<Step> = axes.iter().copied().filter(|a| a.is_sweep()).map(Step::Sweep).collect();
    if axes.contains(&Axis::Gpu) {
        if gpu == GpuPaths::None {
            v.push(Step::Skip("gpu (--gpu none)".into()));
        }
        if matches!(gpu, GpuPaths::Wsl | GpuPaths::Both) {
            let display = (pre.display)();
            v.push(Step::GpuWsl { windowed: display.is_none() });
            if let Some(why) = display {
                v.push(Step::Skip(format!("gpu wsl windowed runs ({why}): offscreen only")));
            }
        }
        if matches!(gpu, GpuPaths::Windows | GpuPaths::Both) {
            v.push(match (pre.windows)() {
                Some(why) => Step::Skip(format!("gpu windows ({why})")),
                None => Step::GpuWindows,
            });
        }
    }
    if axes.contains(&Axis::Olx) {
        v.push(match (pre.docker)() {
            Some(why) => Step::Skip(format!("olx ({why})")),
            None => Step::Olx,
        });
    }
    v
}

/// Why a measured GPU path is not the GPU: a software rasteriser (a `Cpu`
/// device) drew it (ruling T5-A: the path has no non-CPU adapter).
pub fn adapter_problem(path: &GpuPath) -> Option<String> {
    (path.device_type == "Cpu").then(|| {
        format!(
            "software-only adapter: {} ({}, a CPU rasteriser), no hardware GPU on this path",
            path.adapter, path.backend
        )
    })
}

/// What the steps run on: this machine ([`Here`]) or a test's stand-in.
pub trait Machine {
    fn sweep(&mut self, axis: Axis) -> Sweep;
    /// The GPU paths' scenes (the heavy replay, the levels' replays), once
    /// before the first path.
    fn gpu_scenes(&mut self) -> Result<(), String>;
    fn gpu_wsl(&mut self, windowed: bool) -> Result<GpuPath, String>;
    /// Each backend's path, after the build and the stage (their error skips both).
    fn gpu_windows(&mut self) -> Result<gpu::Backends, String>;
    fn olx(&mut self) -> anyhow::Result<OlxComparison>;
}

/// This machine.
struct Here<'a> {
    cli: &'a Path,
    content: &'a Path,
    plan: &'a Plan,
    budgets: Budgets,
    levels: Vec<gpu::Level>,
}

impl Machine for Here<'_> {
    fn sweep(&mut self, axis: Axis) -> Sweep {
        sweeps::run_axis(self.cli, self.content, axis, self.plan)
    }

    fn gpu_scenes(&mut self) -> Result<(), String> {
        self.levels = gpu::scenes(self.cli, self.content, self.plan)?;
        Ok(())
    }

    fn gpu_wsl(&mut self, windowed: bool) -> Result<GpuPath, String> {
        gpu::wsl(self.cli, self.content, &self.levels, windowed, self.plan, &self.budgets)
    }

    fn gpu_windows(&mut self) -> Result<gpu::Backends, String> {
        gpu::windows(self.content, &self.levels, self.plan, &self.budgets)
    }

    fn olx(&mut self) -> anyhow::Result<OlxComparison> {
        olx::run(self.cli, self.content, self.plan)
    }
}

/// Runs `steps` on `m` into `env`; a part that cannot run, or fails, is a
/// `skip` notice and the next step goes on. Gives the notices.
pub fn execute(steps: &[Step], m: &mut dyn Machine, env: &mut Envelope) -> Vec<String> {
    let mut notices = Vec::new();
    let mut skip = |notice: String| {
        println!("skip {notice}");
        notices.push(notice);
    };
    let mut scenes: Option<Result<(), String>> = None;
    let gpu_path = |key: String, r: Result<GpuPath, String>, env: &mut Envelope| -> Result<(), String> {
        let path = r.map_err(|e| gpu::last_line(&e).to_string())?;
        if let Some(why) = adapter_problem(&path) {
            return Err(why);
        }
        env.gpu.insert(key, path);
        Ok(())
    };
    for step in steps {
        if matches!(step, Step::GpuWsl { .. } | Step::GpuWindows) {
            if scenes.is_none() {
                println!("== gpu");
            }
            if let Err(why) = scenes.get_or_insert_with(|| m.gpu_scenes()) {
                let what = if matches!(step, Step::GpuWsl { .. }) { "wsl" } else { "windows" };
                skip(format!("gpu {what} ({why})"));
                continue;
            }
        }
        match step {
            Step::Sweep(axis) => {
                println!("== {} sweep", axis.name());
                env.sweeps.insert(axis.name().into(), m.sweep(*axis));
            }
            Step::GpuWsl { windowed } => {
                let r = m.gpu_wsl(*windowed);
                if let Err(why) = gpu_path("wsl-gl".into(), r, env) {
                    skip(format!("gpu wsl ({why})"));
                }
            }
            Step::GpuWindows => match m.gpu_windows() {
                Err(e) => skip(format!("gpu windows ({})", gpu::last_line(&e))),
                Ok(paths) => {
                    for (backend, r) in paths {
                        if let Err(why) = gpu_path(format!("windows-{backend}"), r, env) {
                            skip(format!("gpu windows {backend} ({why})"));
                        }
                    }
                }
            },
            Step::Olx => {
                println!("== olx");
                match m.olx() {
                    Ok(c) => env.olx = Some(c),
                    Err(e) => skip(format!("olx ({e:#})")),
                }
            }
            Step::Skip(why) => skip(why.clone()),
        }
    }
    notices
}

pub fn run(o: &Opts) -> anyhow::Result<bool> {
    if o.report {
        let out = report::write(o.from.as_deref())?;
        println!("wrote {}", out.display());
        return Ok(true);
    }
    if let Some(reason) = &o.bless {
        anyhow::ensure!(!reason.trim().is_empty(), "--reason must say why the results change");
        anyhow::ensure!(
            !o.quick,
            "--bless writes the baseline from a full run (3 runs a point); --quick results are never blessed"
        );
    }
    let content = root().join("content/packs");
    anyhow::ensure!(
        content.join("olx-core").is_dir(),
        "no content in {} (`cargo xtask fetch-olx-content`)",
        content.display()
    );
    let host = Host::here();
    let plan = Plan::new(o.quick);
    let axes: Vec<Axis> = if o.only.is_empty() { Axis::ALL.to_vec() } else { o.only.clone() };
    println!(
        "envelope ({}): keep this PC otherwise idle; load average now {:.2}. About {} minutes warm (the first Windows build adds a few).",
        if plan.quick { "quick" } else { "full" },
        results::load_average().unwrap_or(0.0),
        if plan.quick { 2 } else { 7 }
    );
    sweeps::build_release(&["rl-cli"]).map_err(anyhow::Error::msg)?;
    let cli = root().join("target/release/rl-cli");
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let mut env = Envelope {
        note: "`cargo xtask envelope` (plan 8): medians of `runs` runs; valid on `host` only.".into(),
        host: host.clone(),
        date: crate::bless::date(secs),
        quick: plan.quick,
        runs: plan.runs,
        seconds: plan.seconds,
        ..Envelope::default()
    };
    let mut here =
        Here { cli: &cli, content: &content, plan: &plan, budgets: env.budgets, levels: Vec::new() };
    let notices = execute(&steps(&axes, o.gpu, &Prereqs::here()), &mut here, &mut env);
    results::finish(&mut env);
    results::write(&root().join(results::LATEST), &env)?;
    for (axis, s) in &env.sweeps {
        let b = |x: &Option<results::Breaking>| {
            x.as_ref().map_or("holds everywhere".into(), |b| {
                format!("breaks at {} ({})", b.value, b.causes.join("; "))
            })
        };
        println!("{axis}: at 144 Hz {}; at 60 Hz {}", b(&s.breaking_144), b(&s.breaking_60));
    }
    for n in &notices {
        println!("skipped: {n}");
    }
    let path = results::path_for(&host);
    let stored = results::load(&root().join(&path)).map_err(anyhow::Error::msg)?;
    match &stored {
        Some(s) if crate::bench::host_notice(&s.host, &host).is_some() => println!(
            "notice: {path} is from another machine ({} CPUs, {:?}, {:?}; here {:?}): not compared",
            s.host.cpus, s.host.cpu, s.host.adapter, host.adapter
        ),
        Some(s) if results::incomparable(s, &env).is_some() => {
            println!("notice: {path}: {}", results::incomparable(s, &env).unwrap_or_default());
        }
        Some(s) => {
            let flags = results::compare(s, &env);
            for f in &flags {
                println!("{}", f.line());
            }
            let n = flags.iter().filter(|f| f.flagged()).count();
            println!("{n} of {} measurements beyond their noise (flagged with !, not failed)", flags.len());
        }
        None => println!(
            "notice: no results for this host yet ({path}); `--bless --reason \"<why>\"` writes them"
        ),
    }
    if let Some(reason) = &o.bless {
        // Only the same host's results of the same kind are kept.
        let same_host = stored.filter(|s| {
            crate::bench::host_notice(&s.host, &host).is_none() && results::incomparable(s, &env).is_none()
        });
        let merged = results::merged(same_host, env);
        if !merged.kept.is_empty() {
            println!("kept from before (not measured now): {:?}", merged.kept);
        }
        let file = root().join(&path);
        let before = std::fs::read(&file).ok();
        results::write(&file, &merged)?;
        let changed = if std::fs::read(&file).ok() != before { vec![path] } else { vec![] };
        crate::bless::log_blessing("envelope", reason, &changed)?;
    }
    println!("results: {}", results::LATEST);
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A machine where every part measures, unless a test says otherwise.
    #[derive(Default)]
    struct Fake {
        scenes: Option<String>,
        wsl: Option<GpuPath>,
        dx12: Option<GpuPath>,
        windowed: Vec<bool>,
        ran: Vec<String>,
    }

    fn hardware() -> GpuPath {
        GpuPath { adapter: "RTX".into(), device_type: "DiscreteGpu".into(), ..GpuPath::default() }
    }

    impl Machine for Fake {
        fn sweep(&mut self, axis: Axis) -> Sweep {
            self.ran.push(axis.name().into());
            Sweep { axis: axis.name().into(), ..Sweep::default() }
        }
        fn gpu_scenes(&mut self) -> Result<(), String> {
            self.ran.push("scenes".into());
            self.scenes.clone().map_or(Ok(()), Err)
        }
        fn gpu_wsl(&mut self, windowed: bool) -> Result<GpuPath, String> {
            self.windowed.push(windowed);
            Ok(self.wsl.clone().unwrap_or_else(hardware))
        }
        fn gpu_windows(&mut self) -> Result<gpu::Backends, String> {
            Ok(vec![("dx12", Ok(self.dx12.clone().unwrap_or_else(hardware))), ("vulkan", Ok(hardware()))])
        }
        fn olx(&mut self) -> anyhow::Result<OlxComparison> {
            self.ran.push("olx".into());
            Ok(OlxComparison::default())
        }
    }

    fn all_here() -> Prereqs {
        Prereqs { windows: || None, docker: || None, display: || None }
    }

    /// Runs everything on `m` with `pre`: the notices and the results.
    fn run_all(pre: &Prereqs, m: &mut Fake) -> (Vec<String>, Envelope) {
        let mut env = Envelope::default();
        let notices = execute(&steps(&Axis::ALL, GpuPaths::Both, pre), m, &mut env);
        (notices, env)
    }

    /// Review Focus 2, rulings PF5 and T5-A: each prerequisite missing is a
    /// `skip` notice naming why, and every other part still runs.
    #[test]
    fn prerequisites_are_notices() {
        let (notices, env) = run_all(&all_here(), &mut Fake::default());
        assert!(notices.is_empty(), "{notices:?}");
        assert_eq!(env.gpu.keys().collect::<Vec<_>>(), ["windows-dx12", "windows-vulkan", "wsl-gl"]);
        assert!(env.olx.is_some() && env.sweeps.len() == 5);

        // Windows interop, or the rustup target (both `winbuild::missing`).
        let windows: [fn() -> Option<String>; 2] = [
            || Some("no Windows interop (cmd.exe is missing)".to_string()),
            || Some("the Rust target is missing: run `rustup target add x86_64-pc-windows-gnu`".to_string()),
        ];
        for why in windows {
            let (notices, env) = run_all(&Prereqs { windows: why, ..all_here() }, &mut Fake::default());
            assert_eq!(notices, [format!("gpu windows ({})", why().unwrap())]);
            assert_eq!(env.gpu.keys().collect::<Vec<_>>(), ["wsl-gl"]);
            assert!(env.olx.is_some() && env.sweeps.len() == 5);
        }

        // Docker.
        let docker = || Some("Docker is not running (`docker info`: Cannot connect)".to_string());
        let (notices, env) = run_all(&Prereqs { docker, ..all_here() }, &mut Fake::default());
        assert_eq!(notices, ["olx (Docker is not running (`docker info`: Cannot connect))"]);
        assert!(env.olx.is_none() && env.gpu.len() == 3 && env.sweeps.len() == 5);

        // No display: the WSL path offscreen only.
        let display = || Some("no display (DISPLAY and WAYLAND_DISPLAY are unset)".to_string());
        let mut m = Fake::default();
        let (notices, env) = run_all(&Prereqs { display, ..all_here() }, &mut m);
        assert_eq!(
            notices,
            ["gpu wsl windowed runs (no display (DISPLAY and WAYLAND_DISPLAY are unset)): offscreen only"]
        );
        assert_eq!((m.windowed, env.gpu.len()), (vec![false], 3));
        assert!(gpu::has_display(Some(":0"), None) && gpu::has_display(None, Some("wayland-0")));
        assert!(!gpu::has_display(None, None) && !gpu::has_display(Some(""), None));

        // A software-only adapter (DX12 always lists Microsoft's Basic
        // Render Driver; only a path with no other adapter draws on it).
        let cpu = |name: &str| GpuPath {
            adapter: name.into(),
            backend: "Dx12".into(),
            device_type: "Cpu".into(),
            ..GpuPath::default()
        };
        let mut m = Fake {
            dx12: Some(cpu("Microsoft Basic Render Driver")),
            wsl: Some(cpu("llvmpipe")),
            ..Fake::default()
        };
        let (notices, env) = run_all(&all_here(), &mut m);
        assert_eq!(notices.len(), 2, "{notices:?}");
        assert!(notices[0].starts_with("gpu wsl (software-only adapter: llvmpipe"), "{notices:?}");
        assert!(
            notices[1].starts_with("gpu windows dx12 (software-only adapter: Microsoft Basic Render Driver")
        );
        assert_eq!(env.gpu.keys().collect::<Vec<_>>(), ["windows-vulkan"]);
        assert!(env.olx.is_some() && env.sweeps.len() == 5);

        // The scenes failing skips both GPU paths, nothing else.
        let mut m = Fake { scenes: Some("the heavy replay failed: x".into()), ..Fake::default() };
        let (notices, env) = run_all(&all_here(), &mut m);
        assert_eq!(
            notices,
            ["gpu wsl (the heavy replay failed: x)", "gpu windows (the heavy replay failed: x)"]
        );
        assert_eq!(m.ran.iter().filter(|r| *r == "scenes").count(), 1, "recorded once");
        assert!(env.gpu.is_empty() && env.olx.is_some() && env.sweeps.len() == 5);

        // The WSL GPU's environment.
        let e = gpu::wsl_env(true);
        assert!(e.contains(&("GALLIUM_DRIVER".into(), "d3d12".into())) && e.len() == 3);
        assert_eq!(gpu::wsl_env(false).len(), 2);
    }

    /// Ruling T7-A: a quick run is never the baseline.
    #[test]
    fn quick_results_are_never_blessed() {
        let o = Opts {
            quick: true,
            only: vec![],
            gpu: GpuPaths::None,
            bless: Some("new machine".into()),
            report: false,
            from: None,
        };
        let e = run(&o).unwrap_err().to_string();
        assert!(e.contains("--quick results are never blessed"), "{e}");
    }

    #[test]
    fn only_the_parts_asked_for_run() {
        let s = steps(&[Axis::Olx, Axis::Worms], GpuPaths::Both, &all_here());
        assert_eq!(s, [Step::Sweep(Axis::Worms), Step::Olx]);
        let s = steps(&[Axis::Gpu], GpuPaths::Wsl, &all_here());
        assert_eq!(s, [Step::GpuWsl { windowed: true }]);
        assert_eq!(steps(&[Axis::Gpu], GpuPaths::None, &all_here()), [Step::Skip("gpu (--gpu none)".into())]);
    }

    #[test]
    fn plans_are_within_the_limits() {
        for quick in [false, true] {
            let p = Plan::new(quick);
            assert_eq!(p.quick, quick);
            assert!(p.worms.iter().all(|&w| (2..=32).contains(&w)), "MAX_WORMS is 32");
            assert!(p.maps.iter().all(|&(w, h)| w <= 4096 && h <= 4096), "OLX's largest side");
            assert!(p.arts.iter().all(|&a| a <= 8), "MAX_ART_SCALE");
            assert!(
                p.runs >= 1 && p.level_start + p.level_frames < (p.seconds * 100.0) as u32,
                "inside the level replays"
            );
        }
        assert_eq!(Plan::new(false).runs, 3);
    }
}
