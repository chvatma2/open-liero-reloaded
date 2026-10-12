//! The GPU paths (spec §5): the heavy replay and the map and art points'
//! replays drawn by `rl-cli gpu-bench` (offscreen, hardware adapter,
//! timestamps when there are any) and played windowed and uncapped by
//! `rl-game --frame-log`, on WSL (GL over Mesa's d3d12 driver) and on
//! Windows (DX12 and Vulkan, through interop). A part that cannot run here
//! is a notice (Review Focus 2, ruling PF5: `super::execute`); nothing
//! here fails the command.
//!
//! A software-only adapter is never measured: `gpu-bench --adapter
//! hardware` takes a non-CPU adapter whenever there is one (DX12 always
//! lists Microsoft's Basic Render Driver beside the real GPU, ruling T5-A)
//! and fails when there is none, and `super::adapter_problem` refuses a
//! path whose report still names a `Cpu` device.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use super::Plan;
use super::results::{self, Budgets, FrameLogOut, GpuLevel, GpuPath, GpuReportOut, SizeOut, Windowed};
use super::sweeps::{self, REPLAYS};
use crate::{root, winbuild};

const HEAVY: &str = "heavy.rlrp";

/// The environment that puts WSL's GL on the real GPU (as `rl-game` sets
/// for itself), naming NVIDIA's adapter when its WSL driver is there.
pub fn wsl_env(nvidia: bool) -> Vec<(String, String)> {
    let mut v =
        vec![("GALLIUM_DRIVER".to_string(), "d3d12".to_string()), ("WGPU_BACKEND".into(), "gl".into())];
    if nvidia {
        v.push(("MESA_D3D12_DEFAULT_ADAPTER_NAME".into(), "NVIDIA".into()));
    }
    v
}

/// Whether a window can open here (WSLg sets both).
pub fn has_display(display: Option<&str>, wayland: Option<&str>) -> bool {
    display.is_some_and(|d| !d.is_empty()) || wayland.is_some_and(|d| !d.is_empty())
}

fn nvidia() -> bool {
    ["libcuda.so.1", "nvidia-smi"].iter().any(|f| Path::new("/usr/lib/wsl/lib").join(f).exists())
}

/// A sweep point's replay: its axis, its value and the file.
pub struct Level {
    pub axis: String,
    pub value: String,
    pub file: PathBuf,
}

/// Where a path's tools run.
enum Runner<'a> {
    Wsl { cli: &'a Path, game: PathBuf, content: &'a Path },
    Windows { stage: &'a winbuild::Stage, backend: &'static str },
}

/// The tool's output and the file it wrote, read as RON.
fn read_ron<T: serde::de::DeserializeOwned>(what: &str, out: &Output, file: &Path) -> Result<T, String> {
    if !out.status.success() {
        return Err(format!(
            "{what} failed ({}): {}{}",
            out.status,
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    let text = std::fs::read_to_string(file).map_err(|e| format!("{}: {e}", file.display()))?;
    ron::from_str(&text).map_err(|e| format!("{}: {e}", file.display()))
}

impl Runner<'_> {
    fn name(&self) -> String {
        match self {
            Runner::Wsl { .. } => "wsl-gl".into(),
            Runner::Windows { backend, .. } => format!("windows-{backend}"),
        }
    }

    fn replay(&self, name: &str) -> String {
        match self {
            Runner::Wsl { .. } => root().join(REPLAYS).join(name).display().to_string(),
            Runner::Windows { .. } => format!("replays/{name}"),
        }
    }

    fn gpu_bench(&self, mut args: Vec<String>) -> Result<GpuReportOut, String> {
        args.insert(0, "gpu-bench".into());
        match self {
            Runner::Wsl { cli, content, .. } => {
                let file = root().join("target/envelope/gpu.ron");
                let _ = std::fs::remove_file(&file);
                args.extend([
                    "--content".into(),
                    content.display().to_string(),
                    "--out".into(),
                    file.display().to_string(),
                ]);
                let mut c = Command::new(cli);
                c.current_dir(root()).args(&args);
                for (k, v) in wsl_env(nvidia()) {
                    c.env(k, v);
                }
                let out = c.output().map_err(|e| format!("cannot run rl-cli: {e}"))?;
                read_ron("rl-cli gpu-bench", &out, &file)
            }
            Runner::Windows { stage, backend } => {
                let file = stage.dir.join("gpu.ron");
                let _ = std::fs::remove_file(&file);
                args.extend(sv(&["--content", "content", "--backend", backend, "--out", "gpu.ron"]));
                let out = winbuild::run(stage, "rl-cli.exe", &args, &[], winbuild::RUN_TIMEOUT)?;
                read_ron("rl-cli.exe gpu-bench", &out, &file)
            }
        }
    }

    fn windowed(&self, size: (u32, u32), seconds: f64) -> Result<FrameLogOut, String> {
        let script = format!("0 size {} {}\n", size.0, size.1);
        let common = |replay: &str, log: &str, script_file: &str, content: &str, data: &str| {
            // `--frame-log-skip` is seconds, which rl-game turns into ticks at
            // 100 a second: the replay plays at 1× here, so they are the
            // window's first 3 wall-clock seconds too (its settling, the size
            // change).
            sv(&[
                "--replay",
                replay,
                "--uncapped",
                "--no-sound",
                "--autoquit-after",
                &seconds.to_string(),
                "--frame-log",
                log,
                "--frame-log-skip",
                "3",
                "--script",
                script_file,
                "--content",
                content,
                "--data-dir",
                data,
            ])
        };
        match self {
            Runner::Wsl { game, content, .. } => {
                let dir = root().join("target/envelope");
                let (log, sf) = (dir.join("frame-log.ron"), dir.join("size.txt"));
                let _ = std::fs::remove_file(&log);
                std::fs::write(&sf, script).map_err(|e| e.to_string())?;
                let mut args = common(
                    &self.replay(HEAVY),
                    &log.display().to_string(),
                    &sf.display().to_string(),
                    &content.display().to_string(),
                    &dir.join("game-data").display().to_string(),
                );
                args.extend(["--window-system".into(), "x11".into()]);
                let mut c = Command::new(game);
                // The game puts itself on the GPU (wsl.rs) only when these are unset.
                c.current_dir(root()).args(&args).env_remove("WGPU_BACKEND").env_remove("GALLIUM_DRIVER");
                let out = c.output().map_err(|e| format!("cannot run rl-game: {e}"))?;
                read_ron("rl-game", &out, &log)
            }
            Runner::Windows { stage, backend } => {
                let log = stage.dir.join("frame-log.ron");
                let _ = std::fs::remove_file(&log);
                std::fs::write(stage.dir.join("size.txt"), script).map_err(|e| e.to_string())?;
                let args = common(&self.replay(HEAVY), "frame-log.ron", "size.txt", "content", "game-data");
                let env = [("WGPU_BACKEND", *backend)];
                let out = winbuild::run(stage, "rl-game.exe", &args, &env, winbuild::RUN_TIMEOUT)?;
                read_ron("rl-game.exe", &out, &log)
            }
        }
    }
}

/// The heavy match (MW 1.0, 32 chaos bots, a 20 000 pool), recorded once.
fn heavy_replay(cli: &Path, content: &Path, plan: &Plan) -> Result<PathBuf, String> {
    let file = root().join(REPLAYS).join(HEAVY);
    std::fs::create_dir_all(root().join(REPLAYS)).map_err(|e| e.to_string())?;
    let out = Command::new(cli)
        .current_dir(root())
        .args(["stress", "--mod", "olx-mw-1-0", "--level", "olx-level-castlestrike", "--worms", "32"])
        .args([
            "--pool",
            "20000",
            "--seconds",
            &plan.heavy_seconds.to_string(),
            "--render-every",
            "0",
            "--no-verify",
        ])
        .arg("--content")
        .arg(content)
        .arg("--record")
        .arg(&file)
        .output()
        .map_err(|e| format!("cannot run rl-cli: {e}"))?;
    if out.status.success() { Ok(file) } else { Err(sweeps::failure("rl-cli stress --record", &out)) }
}

/// The scenes: the heavy replay, recorded now, and the map and art points'
/// replays the sweeps recorded (this run's or an earlier one's).
pub fn scenes(cli: &Path, content: &Path, plan: &Plan) -> Result<Vec<Level>, String> {
    heavy_replay(cli, content, plan).map_err(|e| format!("the heavy replay failed: {}", last_line(&e)))?;
    let mut v = Vec::new();
    for axis in [super::Axis::Map, super::Axis::Art] {
        for val in sweeps::values(axis, plan) {
            let file = root().join(REPLAYS).join(sweeps::replay_name(axis.name(), &val.label));
            if file.is_file() {
                v.push(Level { axis: axis.name().to_string(), value: val.label, file });
            }
        }
    }
    Ok(v)
}

/// Owned arguments.
fn sv(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

fn join<T: ToString>(v: &[T]) -> String {
    v.iter().map(T::to_string).collect::<Vec<_>>().join(",")
}

/// The last non-empty line of an error (a tool's stderr ends with its cause).
pub fn last_line(e: &str) -> &str {
    e.lines().rfind(|l| !l.trim().is_empty()).unwrap_or(e).trim()
}

fn measure(
    r: &Runner,
    levels: &[Level],
    windowed: bool,
    plan: &Plan,
    b: &Budgets,
) -> Result<GpuPath, String> {
    let sizes = join(&plan.sizes.iter().map(|(w, h)| format!("{w}x{h}")).collect::<Vec<_>>());
    let mut runs = Vec::new();
    for i in 0..plan.runs {
        println!("  {} offscreen, run {}", r.name(), i + 1);
        runs.push(r.gpu_bench(sv(&[
            "--replay",
            &r.replay(HEAVY),
            "--adapter",
            "hardware",
            "--sizes",
            &sizes,
            "--start-tick",
            &plan.start_tick.to_string(),
            "--frames",
            &plan.gpu_frames.to_string(),
            "--probe-passes",
            &join(&plan.probe_passes),
            "--probe-frames",
            &plan.probe_frames.to_string(),
        ]))?);
    }
    let first = runs.first().ok_or("no offscreen runs")?;
    let mut path = GpuPath {
        adapter: first.adapter.clone(),
        backend: first.backend.clone(),
        device_type: first.device_type.clone(),
        timestamps: first.timestamps,
        ..GpuPath::default()
    };
    if let Some(why) = super::adapter_problem(&path) {
        return Err(why);
    }
    if !first.timestamps {
        path.notes.push("no timestamp queries on this adapter: CPU-side frame times only".into());
    }
    for (i, _) in plan.sizes.iter().enumerate() {
        let per: Vec<SizeOut> = runs.iter().filter_map(|x| x.sizes.get(i).cloned()).collect();
        path.sizes.push(results::gpu_size(&per, b));
    }
    for l in levels {
        let name = l.file.file_name().unwrap_or_default().to_string_lossy().to_string();
        println!("  {} level {} {}", r.name(), l.axis, l.value);
        let args = sv(&[
            "--replay",
            &r.replay(&name),
            "--adapter",
            "hardware",
            "--sizes",
            "2560x1440",
            "--start-tick",
            &plan.level_start.to_string(),
            "--frames",
            &plan.level_frames.to_string(),
            "--probe-passes",
            "0",
            "--probe-frames",
            "1",
        ]);
        match r.gpu_bench(args) {
            Ok(rep) => {
                if let Some(s) = rep.sizes.first() {
                    path.levels.push(GpuLevel {
                        axis: l.axis.clone(),
                        value: l.value.clone(),
                        size: s.size,
                        total_ms: s.total_ms,
                        gpu_ms: s.gpu_ms,
                        gpu_dropped: s.gpu_dropped,
                    });
                }
            }
            Err(e) => path.notes.push(format!("{} {}: {}", l.axis, l.value, last_line(&e))),
        }
    }
    if !windowed {
        path.notes.push("no display: windowed runs skipped".into());
        return Ok(path);
    }
    for &size in &plan.sizes {
        println!("  {} windowed {}x{}", r.name(), size.0, size.1);
        match r.windowed(size, plan.windowed_seconds) {
            Ok(log) => path.windowed.push(Windowed {
                requested: size,
                size: log.size,
                reached: log.size == size,
                gpu: log.gpu,
                frames: log.frames,
                fps: log.fps,
                frame_ms: log.frame_ms,
                busy_ms: log.busy_ms,
            }),
            Err(e) => path.notes.push(format!("windowed {}x{}: {}", size.0, size.1, last_line(&e))),
        }
    }
    Ok(path)
}

/// The WSL path, `windowed` when a window can open here.
pub fn wsl(
    cli: &Path,
    content: &Path,
    levels: &[Level],
    windowed: bool,
    plan: &Plan,
    b: &Budgets,
) -> Result<GpuPath, String> {
    if windowed {
        sweeps::build_release(&["rl-game"])
            .map_err(|e| format!("rl-game does not build: {}", last_line(&e)))?;
    }
    let r = Runner::Wsl { cli, game: root().join("target/release/rl-game"), content };
    measure(&r, levels, windowed, plan, b)
}

/// Each Windows backend's path, or why it could not be measured.
pub type Backends = Vec<(&'static str, Result<GpuPath, String>)>;

/// The Windows paths: built and staged once (an error skips both), then
/// each backend measured on its own.
pub fn windows(content: &Path, levels: &[Level], plan: &Plan, b: &Budgets) -> Result<Backends, String> {
    println!("  building for Windows (the first build takes minutes)");
    let files: Vec<PathBuf> = std::iter::once(root().join(REPLAYS).join(HEAVY))
        .chain(levels.iter().map(|l| l.file.clone()))
        .collect();
    let stage = winbuild::build().and_then(|rel| winbuild::stage(&rel, content, &files))?;
    Ok(["dx12", "vulkan"]
        .into_iter()
        .map(|backend| (backend, measure(&Runner::Windows { stage: &stage, backend }, levels, true, plan, b)))
        .collect())
}
