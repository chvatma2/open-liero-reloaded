//! The native Windows build (plan 8): `olr-cli` and `olr-game`
//! cross-compiled for `x86_64-pc-windows-gnu` with mingw-w64, staged with
//! the content in Windows' temp directory (`%TEMP%\olr-envelope`)
//! and run from WSL through interop: nothing is installed on Windows.
//! Interop forwards only the variables `WSLENV` names, so `run` adds them.
//! A run that outlives its timeout is stopped on the Windows side too, by
//! image path under the stage (never by image name: the user's own
//! processes are never touched).

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use crate::root;

pub const TARGET: &str = "x86_64-pc-windows-gnu";
pub const LINKER: &str = "x86_64-w64-mingw32-gcc";
pub const OBJDUMP: &str = "x86_64-w64-mingw32-objdump";
/// `appendWindowsPath` may be off: `cmd.exe` by its full path.
pub const CMD: &str = "/mnt/c/Windows/System32/cmd.exe";
/// Windows PowerShell (part of Windows), to stop a run's processes.
pub const POWERSHELL: &str = "/mnt/c/Windows/System32/WindowsPowerShell/v1.0/powershell.exe";
/// The variable that hands the stage's Windows path to `STOP_SCRIPT`.
pub const STAGE_VAR: &str = "OLR_STAGE";
/// Force-stops every process whose image lies under `$env:OLR_STAGE` and
/// prints how many: matched by path prefix, never by image name, so the
/// user's own processes are never touched. The path stays out of the
/// script's source (PowerShell takes `'` and U+2018-U+201B as quotes).
pub const STOP_SCRIPT: &str = "if (-not $env:OLR_STAGE) { exit 2 }; \
    $p = $env:OLR_STAGE + '\\'; \
    $s = @(Get-Process | Where-Object { $_.Path -and $_.Path.StartsWith($p, [StringComparison]::OrdinalIgnoreCase) }); \
    $s | Stop-Process -Force; $s.Count";
/// A generous bound for one run (an envelope point takes a minute or two):
/// past it `run` stops the process and fails.
pub const RUN_TIMEOUT: Duration = Duration::from_secs(10 * 60);
pub const EXES: [&str; 3] = ["olr-cli.exe", "olr.exe", "olr-detcheck.exe"];
/// mingw's runtime DLLs an exe may import; copied beside it when it does.
pub const MINGW_DLLS: [&str; 3] = ["libwinpthread-1.dll", "libgcc_s_seh-1.dll", "libstdc++-6.dll"];
/// The stage's directory name in `%TEMP%`.
pub const STAGE: &str = "olr-envelope";

fn tail(text: &str, n: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(n)..].join("\n")
}

/// Why the Windows path cannot run here, if it cannot (a notice names the fix).
pub fn missing() -> Option<String> {
    interop_missing().or_else(build_missing)
}

/// Why Windows programs cannot run here through WSL interop, if they cannot.
pub fn interop_missing() -> Option<String> {
    if !Path::new(CMD).exists() {
        return Some(format!("no Windows interop ({CMD} is missing)"));
    }
    // Newer WSL registers interop as `WSLInterop-late`.
    let enabled = ["WSLInterop", "WSLInterop-late"].iter().any(|name| {
        std::fs::read_to_string(Path::new("/proc/sys/fs/binfmt_misc").join(name))
            .is_ok_and(|s| s.starts_with("enabled"))
    });
    if !enabled {
        return Some(
            "Windows interop is disabled (/proc/sys/fs/binfmt_misc/WSLInterop or WSLInterop-late)".into(),
        );
    }
    if !Path::new(POWERSHELL).exists() {
        return Some(format!("no Windows PowerShell ({POWERSHELL} is missing)"));
    }
    None
}

/// Why the Windows build cannot be made here (the Rust target, mingw-w64), if it cannot.
pub fn build_missing() -> Option<String> {
    let installed = Command::new("rustup")
        .current_dir(root())
        .args(["target", "list", "--installed"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();
    if !installed.lines().any(|l| l.trim() == TARGET) {
        return Some(format!(
            "the Rust target is missing: run `rustup target add {TARGET}` in the repository"
        ));
    }
    for tool in [LINKER, OBJDUMP] {
        if Command::new(tool).arg("--version").output().is_err() {
            return Some(format!("no {tool}: `sudo apt install mingw-w64`"));
        }
    }
    None
}

/// Builds `olr-cli`, `olr-game` and `olr-detcheck` for Windows (release); gives their directory.
pub fn build() -> Result<PathBuf, String> {
    let out = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
        .current_dir(root())
        .args([
            "build",
            "--quiet",
            "--release",
            "--target",
            TARGET,
            "-p",
            "olr-cli",
            "-p",
            "olr-game",
            "-p",
            "olr-detcheck",
        ])
        .env("CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER", LINKER)
        .output()
        .map_err(|e| format!("cannot run cargo: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "the Windows build failed:\n{}",
            tail(&String::from_utf8_lossy(&out.stderr), 40)
        ));
    }
    Ok(root().join("target").join(TARGET).join("release"))
}

/// The DLLs `objdump -p` says an exe imports.
pub fn imported_dlls(objdump: &str) -> Vec<String> {
    objdump.lines().filter_map(|l| l.trim().strip_prefix("DLL Name:")).map(|n| n.trim().to_string()).collect()
}

/// `C:\Users\x` as `/mnt/c/Users/x` (`None` for anything but a drive path).
pub fn to_wsl_path(win: &str) -> Option<PathBuf> {
    let win = win.trim();
    let (drive, rest) = win.split_once(":\\").or_else(|| win.split_once(":/"))?;
    let mut d = drive.chars();
    let letter = d.next().filter(|c| c.is_ascii_alphabetic() && d.next().is_none())?;
    Some(PathBuf::from(format!("/mnt/{}/{}", letter.to_ascii_lowercase(), rest.replace('\\', "/"))))
}

/// `existing` `WSLENV` with `names` added once each.
pub fn wslenv(existing: Option<&str>, names: &[&str]) -> String {
    let mut parts: Vec<String> =
        existing.unwrap_or("").split(':').filter(|s| !s.is_empty()).map(String::from).collect();
    for n in names {
        if !parts.iter().any(|p| p.split('/').next() == Some(*n)) {
            parts.push((*n).to_string());
        }
    }
    parts.join(":")
}

/// Windows' `%TEMP%`, through `cmd.exe` started in `/mnt/c` (a Linux
/// working directory would make it warn about UNC paths).
pub fn windows_temp() -> Result<PathBuf, String> {
    let out = Command::new(CMD)
        .current_dir("/mnt/c")
        .args(["/d", "/c", "echo %TEMP%"])
        .output()
        .map_err(|e| format!("cannot run {CMD}: {e}"))?;
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    to_wsl_path(text.lines().rfind(|l| !l.trim().is_empty()).unwrap_or(""))
        .ok_or_else(|| format!("cmd.exe gave no temp directory: {text:?}"))
}

fn copy(from: &Path, to: &Path) -> Result<(), String> {
    std::fs::copy(from, to)
        .map(drop)
        .map_err(|e| format!("copying {} to {}: {e}", from.display(), to.display()))
}

pub(crate) fn copy_dir(from: &Path, to: &Path) -> Result<(), String> {
    std::fs::create_dir_all(to).map_err(|e| format!("{}: {e}", to.display()))?;
    for e in std::fs::read_dir(from).map_err(|e| format!("{}: {e}", from.display()))? {
        let e = e.map_err(|e| e.to_string())?;
        let (src, dst) = (e.path(), to.join(e.file_name()));
        if src.is_dir() { copy_dir(&src, &dst)? } else { copy(&src, &dst)? }
    }
    Ok(())
}

/// The content's file count, bytes and newest modification time: copied
/// again only when it changes (a same-size edit moves the newest time).
fn content_stamp(dir: &Path) -> Result<String, String> {
    #[derive(Default)]
    struct Sum {
        files: u64,
        bytes: u64,
        newest: Duration,
    }
    fn walk(d: &Path, sum: &mut Sum) -> Result<(), String> {
        for e in std::fs::read_dir(d).map_err(|e| format!("{}: {e}", d.display()))? {
            let p = e.map_err(|e| e.to_string())?.path();
            if p.is_dir() {
                walk(&p, sum)?;
            } else {
                let m = std::fs::metadata(&p).map_err(|e| format!("{}: {e}", p.display()))?;
                let modified = m.modified().map_err(|e| format!("{}: {e}", p.display()))?;
                sum.files += 1;
                sum.bytes += m.len();
                sum.newest =
                    sum.newest.max(modified.duration_since(std::time::UNIX_EPOCH).unwrap_or_default());
            }
        }
        Ok(())
    }
    let mut sum = Sum::default();
    walk(dir, &mut sum)?;
    Ok(format!(
        "{} files, {} bytes, newest {}.{:09}",
        sum.files,
        sum.bytes,
        sum.newest.as_secs(),
        sum.newest.subsec_nanos()
    ))
}

fn mingw_dll(name: &str) -> Option<PathBuf> {
    let mut dirs = vec![PathBuf::from("/usr/x86_64-w64-mingw32/lib")];
    if let Ok(rd) = std::fs::read_dir("/usr/lib/gcc/x86_64-w64-mingw32") {
        let mut v: Vec<PathBuf> = rd.filter_map(|e| e.ok().map(|e| e.path())).collect();
        v.sort();
        dirs.extend(v.into_iter().rev());
    }
    dirs.into_iter().map(|d| d.join(name)).find(|p| p.is_file())
}

/// The mingw runtime DLLs (`MINGW_DLLS`) the Windows exe `exe` imports
/// (`objdump -p`), each with its file in mingw-w64's directories.
pub fn mingw_imports(exe: &Path) -> Result<Vec<(String, PathBuf)>, String> {
    let dump = Command::new(OBJDUMP).arg("-p").arg(exe).output().map_err(|e| format!("{OBJDUMP}: {e}"))?;
    if !dump.status.success() {
        return Err(format!(
            "{OBJDUMP} -p {} failed ({}): {}",
            exe.display(),
            dump.status,
            tail(&String::from_utf8_lossy(&dump.stderr), 5)
        ));
    }
    let mut found = Vec::new();
    for dll in imported_dlls(&String::from_utf8_lossy(&dump.stdout)) {
        if MINGW_DLLS.iter().any(|d| d.eq_ignore_ascii_case(&dll)) {
            let file = mingw_dll(&dll)
                .ok_or_else(|| format!("{} needs {dll}, not in mingw-w64's directories", exe.display()))?;
            found.push((dll, file));
        }
    }
    Ok(found)
}

/// A staged Windows build.
pub struct Stage {
    pub dir: PathBuf,
}

/// Copies the exes (with any mingw DLL they import), the content (when it
/// changed) and `files` (into `replays/`) to `%TEMP%\olr-envelope`.
pub fn stage(release: &Path, content: &Path, files: &[PathBuf]) -> Result<Stage, String> {
    let dir = windows_temp()?.join(STAGE);
    std::fs::create_dir_all(dir.join("replays")).map_err(|e| format!("{}: {e}", dir.display()))?;
    // A run an interrupted xtask left behind would hold the exes.
    let stopped = stop_stage_processes(&dir)?;
    if stopped > 0 {
        println!("  stopped {stopped} process(es) left running from {}", dir.display());
    }
    for exe in EXES {
        let src = release.join(exe);
        copy(&src, &dir.join(exe))?;
        for (dll, found) in mingw_imports(&src)? {
            copy(&found, &dir.join(&dll))?;
        }
    }
    let stamp = content_stamp(content)?;
    let stamp_file = dir.join("content.stamp");
    if std::fs::read_to_string(&stamp_file).ok().as_deref() != Some(stamp.as_str()) {
        println!("  copying the content to {} (once)", dir.display());
        match std::fs::remove_dir_all(dir.join("content")) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                return Err(format!("removing the old content in {}: {e}", dir.display()));
            }
            _ => {}
        }
        copy_dir(content, &dir.join("content"))?;
        std::fs::write(&stamp_file, &stamp).map_err(|e| format!("{}: {e}", stamp_file.display()))?;
    }
    for f in files {
        copy(f, &dir.join("replays").join(f.file_name().unwrap_or_default()))?;
    }
    Ok(Stage { dir })
}

/// `/mnt/c/Users/x` as `C:\Users\x` (`None` outside a `/mnt/<drive>` path).
pub fn to_windows_path(wsl: &Path) -> Option<String> {
    let rest = wsl.to_str()?.strip_prefix("/mnt/")?;
    let (drive, rest) = rest.split_once('/').unwrap_or((rest, ""));
    let mut d = drive.chars();
    let letter = d.next().filter(|c| c.is_ascii_alphabetic() && d.next().is_none())?;
    Some(format!("{}:\\{}", letter.to_ascii_uppercase(), rest.trim_end_matches('/').replace('/', "\\")))
}

/// The value `STOP_SCRIPT` gets in `STAGE_VAR` for the Windows path
/// `stage_win`: `None` unless it names a stage directory (`STAGE`), so a
/// wrong path can never stop a whole drive's processes.
pub fn stop_target(stage_win: &str) -> Option<&str> {
    let dir = stage_win.trim_end_matches('\\');
    (dir.rsplit('\\').next() == Some(STAGE)).then_some(dir)
}

/// Stops whatever runs from the stage directory `dir` (`STOP_SCRIPT`);
/// gives how many processes it stopped.
pub fn stop_stage_processes(dir: &Path) -> Result<usize, String> {
    let win = to_windows_path(dir).ok_or_else(|| format!("{} is not on a Windows drive", dir.display()))?;
    let target = stop_target(&win).ok_or_else(|| format!("{win} is not a {STAGE} directory"))?;
    let out = Command::new(POWERSHELL)
        .current_dir("/mnt/c")
        .args(["-NoProfile", "-NonInteractive", "-Command", STOP_SCRIPT])
        .env(STAGE_VAR, target)
        .env("WSLENV", wslenv(std::env::var("WSLENV").ok().as_deref(), &[STAGE_VAR]))
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("cannot run {POWERSHELL}: {e}"))?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    match stdout.trim().parse() {
        Ok(n) if out.status.success() => Ok(n),
        _ => Err(format!(
            "stopping the processes under {win} failed ({}): {}{}",
            out.status,
            stdout.trim(),
            tail(&String::from_utf8_lossy(&out.stderr), 5)
        )),
    }
}

/// `d` as whole seconds, or milliseconds when it has a fraction.
fn duration_text(d: Duration) -> String {
    if d.subsec_nanos() == 0 { format!("{} s", d.as_secs()) } else { format!("{} ms", d.as_millis()) }
}

fn drain(pipe: Option<impl Read + Send + 'static>) -> std::thread::JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(mut p) = pipe {
            let _ = p.read_to_end(&mut buf);
        }
        buf
    })
}

/// Runs `exe` of `stage` with the stage as its working directory (paths in
/// `args` are relative to it); `env` reaches Windows through `WSLENV`. It
/// fails when the exe exits unsuccessfully (the status in the message:
/// interop keeps a Windows exit code's low 8 bits, so 0xC0000135 shows as
/// 53) or outlives `timeout`: then it is killed and every process under
/// the stage is stopped on the Windows side.
pub fn run(
    stage: &Stage,
    exe: &str,
    args: &[String],
    env: &[(&str, &str)],
    timeout: Duration,
) -> Result<Output, String> {
    run_in(stage, &stage.dir.join(exe), &stage.dir, args, env, timeout)
}

/// [`run`] for the exe at `path` (under the stage) in the working directory `cwd`.
pub fn run_in(
    stage: &Stage,
    path: &Path,
    cwd: &Path,
    args: &[String],
    env: &[(&str, &str)],
    timeout: Duration,
) -> Result<Output, String> {
    let exe = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
    let mut c = Command::new(path);
    c.current_dir(cwd).args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    if !env.is_empty() {
        for (k, v) in env {
            c.env(k, v);
        }
        let names: Vec<&str> = env.iter().map(|(k, _)| *k).collect();
        c.env("WSLENV", wslenv(std::env::var("WSLENV").ok().as_deref(), &names));
    }
    let mut child = c.spawn().map_err(|e| format!("cannot run {exe} through Windows interop: {e}"))?;
    let (stdout, stderr) = (drain(child.stdout.take()), drain(child.stderr.take()));
    let start = Instant::now();
    let status = loop {
        let why = match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if start.elapsed() < timeout => {
                std::thread::sleep(Duration::from_millis(20));
                continue;
            }
            Ok(None) => format!("{exe} did not finish within {}", duration_text(timeout)),
            Err(e) => format!("waiting for {exe}: {e}"),
        };
        // The Windows side first (killing the interop child also ends its
        // Windows process, but not one it started), then the child. The
        // pipes' readers are left behind: a survivor would keep them open.
        let stopped = stop_stage_processes(&stage.dir);
        let _ = child.kill();
        let _ = child.wait();
        return Err(match stopped {
            Ok(n) => format!("{why}; stopped {n} process(es) under the stage"),
            Err(e) => format!("{why}; {e}"),
        });
    };
    let output = Output {
        status,
        stdout: stdout.join().unwrap_or_default(),
        stderr: stderr.join().unwrap_or_default(),
    };
    if !status.success() {
        return Err(format!(
            "{exe} {} failed ({status}):\n{}",
            args.join(" "),
            tail(&String::from_utf8_lossy(&output.stderr), 20)
        ));
    }
    Ok(output)
}

/// `cargo xtask windows`: builds, stages, and lists what each backend sees.
pub fn command() -> anyhow::Result<bool> {
    if let Some(why) = missing() {
        println!("FAIL windows: {why}");
        return Ok(false);
    }
    println!("building olr-cli and olr-game for {TARGET} (release; the first build takes minutes)...");
    let release = build().map_err(anyhow::Error::msg)?;
    let stage = stage(&release, &root().join("content/packs"), &[]).map_err(anyhow::Error::msg)?;
    println!("staged in {}", stage.dir.display());
    let mut ok = true;
    for backend in ["dx12", "vulkan"] {
        let args = ["gpu-bench", "--list-adapters", "--backend", backend].map(String::from);
        match run(&stage, "olr-cli.exe", &args, &[], RUN_TIMEOUT) {
            Ok(out) => println!(
                "{backend}:\n{}{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            ),
            Err(why) => {
                println!("FAIL windows ({backend}): {why}");
                ok = false;
            }
        }
    }
    Ok(ok)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_paths_map_to_mnt() {
        assert_eq!(
            to_wsl_path("C:\\Users\\Martin\\AppData\\Local\\Temp\r\n"),
            Some(PathBuf::from("/mnt/c/Users/Martin/AppData/Local/Temp"))
        );
        assert_eq!(to_wsl_path("D:/x"), Some(PathBuf::from("/mnt/d/x")));
        assert_eq!(to_wsl_path("\\\\wsl.localhost\\x"), None);
        assert_eq!(to_wsl_path(""), None);
    }

    #[test]
    fn imported_dlls_are_read_from_objdump() {
        let dump = "The Import Tables\n\tDLL Name: KERNEL32.dll\n\tvma: x\n\tDLL Name: libwinpthread-1.dll\n";
        assert_eq!(imported_dlls(dump), ["KERNEL32.dll", "libwinpthread-1.dll"]);
    }

    #[test]
    fn stage_paths_map_back_to_windows() {
        assert_eq!(
            to_windows_path(Path::new("/mnt/c/Users/Martin/AppData/Local/Temp/olr-envelope")).as_deref(),
            Some("C:\\Users\\Martin\\AppData\\Local\\Temp\\olr-envelope")
        );
        assert_eq!(to_windows_path(Path::new("/mnt/d")).as_deref(), Some("D:\\"));
        assert_eq!(to_windows_path(Path::new("/home/martin")), None);
        assert_eq!(to_windows_path(Path::new("/mnt/wsl/x")), None);
    }

    #[test]
    fn the_stop_script_targets_only_the_stage() {
        // Only processes whose image lies under the stage, never by image name.
        assert!(STOP_SCRIPT.contains("$_.Path.StartsWith($p, [StringComparison]::OrdinalIgnoreCase)"));
        assert!(STOP_SCRIPT.contains("$p = $env:OLR_STAGE + '\\'"));
        assert!(STOP_SCRIPT.contains("Stop-Process -Force"));
        assert!(!STOP_SCRIPT.contains("olr-") && !STOP_SCRIPT.contains("-Name"));
        // Every quote PowerShell knows in the path: it goes through the
        // variable, never into the script's source.
        for q in ['\'', '\u{2018}', '\u{2019}', '\u{201A}', '\u{201B}'] {
            let path = format!("C:\\Users\\O{q}Neil{q}) }} | % {{ 1 }} #\\Temp\\olr-envelope\\");
            assert_eq!(stop_target(&path), Some(&path[..path.len() - 1]));
            assert!(!STOP_SCRIPT.contains("Neil"), "{q}");
        }
        assert!(!STOP_SCRIPT.contains(['\u{2018}', '\u{2019}', '\u{201A}', '\u{201B}']));
        // Never a drive or any directory that is not a stage.
        assert_eq!(stop_target("C:\\"), None);
        assert_eq!(stop_target("C:\\Users\\Martin\\AppData\\Local\\Temp"), None);
        assert_eq!(stop_target("C:\\x\\olr-envelope-not"), None);
    }

    #[test]
    fn timeouts_read_in_seconds_or_milliseconds() {
        assert_eq!(duration_text(Duration::from_secs(600)), "600 s");
        assert_eq!(duration_text(Duration::from_millis(250)), "250 ms");
        assert_eq!(duration_text(Duration::from_millis(1500)), "1500 ms");
    }

    #[test]
    fn the_content_stamp_sees_a_same_size_edit() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.ron");
        std::fs::write(&file, "one").unwrap();
        let f = std::fs::File::options().write(true).open(&file).unwrap();
        f.set_modified(std::time::UNIX_EPOCH + Duration::from_secs(1_000_000)).unwrap();
        let before = content_stamp(dir.path()).unwrap();
        std::fs::write(&file, "two").unwrap();
        f.set_modified(std::time::UNIX_EPOCH + Duration::from_secs(2_000_000)).unwrap();
        assert_ne!(content_stamp(dir.path()).unwrap(), before);
    }

    /// On the real machine: `cargo nextest run -p xtask winbuild --run-ignored only`
    /// after `cargo xtask windows` (it opens a game window for 5 s).
    #[test]
    #[ignore = "needs Windows interop and the stage `cargo xtask windows` writes"]
    fn runs_fail_on_a_bad_exit_and_are_stopped_past_their_timeout() {
        let stage = Stage { dir: windows_temp().unwrap().join(STAGE) };
        let bad = ["gpu-bench", "--backend", "nope"].map(String::from);
        let err = run(&stage, "olr-cli.exe", &bad, &[], RUN_TIMEOUT).unwrap_err();
        assert!(err.contains("failed (exit status: "), "{err}");

        let game = ["--no-sound", "--content", "content", "--data-dir", "data"].map(String::from);
        let err = run(&stage, "olr.exe", &game, &[], Duration::from_secs(5)).unwrap_err();
        assert!(err.ends_with("did not finish within 5 s; stopped 1 process(es) under the stage"), "{err}");
        // Nothing is left: another stop finds none.
        assert_eq!(stop_stage_processes(&stage.dir), Ok(0));
        // Quotes in the path reach PowerShell through the variable intact.
        let quoted = stage.dir.join("O'N\u{2018}e\u{2019}i\u{201A}l\u{201B}").join(STAGE);
        assert_eq!(stop_stage_processes(&quoted), Ok(0));
    }

    #[test]
    fn wslenv_adds_names_once() {
        assert_eq!(wslenv(None, &["WGPU_BACKEND"]), "WGPU_BACKEND");
        assert_eq!(
            wslenv(Some("WT_SESSION:WGPU_BACKEND/u"), &["WGPU_BACKEND", "X"]),
            "WT_SESSION:WGPU_BACKEND/u:X"
        );
    }
}
