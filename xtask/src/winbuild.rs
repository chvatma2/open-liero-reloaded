//! The native Windows build (plan 8): `rl-cli` and `rl-game`
//! cross-compiled for `x86_64-pc-windows-gnu` with mingw-w64, staged with
//! the content in Windows' temp directory (`%TEMP%\rust-liero-envelope`)
//! and run from WSL through interop: nothing is installed on Windows.
//! Interop forwards only the variables `WSLENV` names, so `run` adds them.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use crate::root;

pub const TARGET: &str = "x86_64-pc-windows-gnu";
pub const LINKER: &str = "x86_64-w64-mingw32-gcc";
pub const OBJDUMP: &str = "x86_64-w64-mingw32-objdump";
/// `appendWindowsPath` may be off: `cmd.exe` by its full path.
pub const CMD: &str = "/mnt/c/Windows/System32/cmd.exe";
pub const EXES: [&str; 2] = ["rl-cli.exe", "rl-game.exe"];
/// mingw's runtime DLLs an exe may import; copied beside it when it does.
pub const MINGW_DLLS: [&str; 3] = ["libwinpthread-1.dll", "libgcc_s_seh-1.dll", "libstdc++-6.dll"];
/// The stage's directory name in `%TEMP%`.
pub const STAGE: &str = "rust-liero-envelope";

fn tail(text: &str, n: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(n)..].join("\n")
}

/// Why the Windows path cannot run here, if it cannot (a notice names the fix).
pub fn missing() -> Option<String> {
    if !Path::new(CMD).exists() {
        return Some(format!("no Windows interop ({CMD} is missing)"));
    }
    // Newer WSL registers interop as `WSLInterop-late`.
    let enabled = ["WSLInterop", "WSLInterop-late"].iter().any(|name| {
        std::fs::read_to_string(Path::new("/proc/sys/fs/binfmt_misc").join(name))
            .is_ok_and(|s| s.starts_with("enabled"))
    });
    if !enabled {
        return Some("Windows interop is disabled (/proc/sys/fs/binfmt_misc/WSLInterop)".into());
    }
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

/// Builds `rl-cli` and `rl-game` for Windows (release); gives their directory.
pub fn build() -> Result<PathBuf, String> {
    let out = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
        .current_dir(root())
        .args(["build", "--quiet", "--release", "--target", TARGET, "-p", "rl-cli", "-p", "rl-game"])
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
        .args(["/c", "echo %TEMP%"])
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

fn copy_dir(from: &Path, to: &Path) -> Result<(), String> {
    std::fs::create_dir_all(to).map_err(|e| format!("{}: {e}", to.display()))?;
    for e in std::fs::read_dir(from).map_err(|e| format!("{}: {e}", from.display()))? {
        let e = e.map_err(|e| e.to_string())?;
        let (src, dst) = (e.path(), to.join(e.file_name()));
        if src.is_dir() { copy_dir(&src, &dst)? } else { copy(&src, &dst)? }
    }
    Ok(())
}

/// The content's file count and bytes: copied again only when it changes.
fn content_stamp(dir: &Path) -> Result<String, String> {
    fn walk(d: &Path, n: &mut u64, bytes: &mut u64) -> Result<(), String> {
        for e in std::fs::read_dir(d).map_err(|e| format!("{}: {e}", d.display()))? {
            let p = e.map_err(|e| e.to_string())?.path();
            if p.is_dir() {
                walk(&p, n, bytes)?;
            } else {
                *n += 1;
                *bytes += std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0);
            }
        }
        Ok(())
    }
    let (mut n, mut bytes) = (0, 0);
    walk(dir, &mut n, &mut bytes)?;
    Ok(format!("{n} files, {bytes} bytes"))
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

/// A staged Windows build.
pub struct Stage {
    pub dir: PathBuf,
}

/// Copies the exes (with any mingw DLL they import), the content (when it
/// changed) and `files` (into `replays/`) to `%TEMP%\rust-liero-envelope`.
pub fn stage(release: &Path, content: &Path, files: &[PathBuf]) -> Result<Stage, String> {
    let dir = windows_temp()?.join(STAGE);
    std::fs::create_dir_all(dir.join("replays")).map_err(|e| format!("{}: {e}", dir.display()))?;
    for exe in EXES {
        let src = release.join(exe);
        copy(&src, &dir.join(exe))?;
        let dump =
            Command::new(OBJDUMP).arg("-p").arg(&src).output().map_err(|e| format!("{OBJDUMP}: {e}"))?;
        for dll in imported_dlls(&String::from_utf8_lossy(&dump.stdout)) {
            if MINGW_DLLS.iter().any(|d| d.eq_ignore_ascii_case(&dll)) {
                let found = mingw_dll(&dll)
                    .ok_or_else(|| format!("{exe} needs {dll}, not in mingw-w64's directories"))?;
                copy(&found, &dir.join(&dll))?;
            }
        }
    }
    let stamp = content_stamp(content)?;
    let stamp_file = dir.join("content.stamp");
    if std::fs::read_to_string(&stamp_file).ok().as_deref() != Some(stamp.as_str()) {
        println!("  copying the content to {} (once)", dir.display());
        let _ = std::fs::remove_dir_all(dir.join("content"));
        copy_dir(content, &dir.join("content"))?;
        std::fs::write(&stamp_file, &stamp).map_err(|e| format!("{}: {e}", stamp_file.display()))?;
    }
    for f in files {
        copy(f, &dir.join("replays").join(f.file_name().unwrap_or_default()))?;
    }
    Ok(Stage { dir })
}

/// Runs `exe` of `stage` with the stage as its working directory (paths in
/// `args` are relative to it); `env` reaches Windows through `WSLENV`.
pub fn run(stage: &Stage, exe: &str, args: &[String], env: &[(&str, &str)]) -> Result<Output, String> {
    let mut c = Command::new(stage.dir.join(exe));
    c.current_dir(&stage.dir).args(args);
    if !env.is_empty() {
        for (k, v) in env {
            c.env(k, v);
        }
        let names: Vec<&str> = env.iter().map(|(k, _)| *k).collect();
        c.env("WSLENV", wslenv(std::env::var("WSLENV").ok().as_deref(), &names));
    }
    c.output().map_err(|e| format!("cannot run {exe} through Windows interop: {e}"))
}

/// `cargo xtask windows`: builds, stages, and lists what each backend sees.
pub fn command() -> anyhow::Result<bool> {
    if let Some(why) = missing() {
        println!("FAIL windows: {why}");
        return Ok(false);
    }
    println!("building rl-cli and rl-game for {TARGET} (release; the first build takes minutes)...");
    let release = build().map_err(anyhow::Error::msg)?;
    let stage = stage(&release, &root().join("content/packs"), &[]).map_err(anyhow::Error::msg)?;
    println!("staged in {}", stage.dir.display());
    for backend in ["dx12", "vulkan"] {
        let args = ["gpu-bench", "--list-adapters", "--backend", backend].map(String::from);
        let out = run(&stage, "rl-cli.exe", &args, &[]).map_err(anyhow::Error::msg)?;
        println!(
            "{backend}:\n{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
    }
    Ok(true)
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
    fn wslenv_adds_names_once() {
        assert_eq!(wslenv(None, &["WGPU_BACKEND"]), "WGPU_BACKEND");
        assert_eq!(
            wslenv(Some("WT_SESSION:WGPU_BACKEND/u"), &["WGPU_BACKEND", "X"]),
            "WT_SESSION:WGPU_BACKEND/u:X"
        );
    }
}
