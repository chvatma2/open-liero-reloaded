//! OLX's frame times for the performance envelope (plan 8): the oracle's
//! patched OLX (Docker) plays a scenario with `ORACLE_TIMING` (each frame's
//! wall time, projectile count and `rand()` calls, to a file of its own:
//! the trace is untouched) and `ORACLE_HEADER_ONLY` (the trace keeps the
//! header and frame -1, which `rl-cli scenario --time` needs to build the
//! same world). OLX runs in a container on WSL's own kernel: no extra
//! virtualisation, the same clock.

use std::path::Path;

use anyhow::Context as _;
use rl_oracle::Scenario;

use crate::{oracle, root};

/// The heavy timing scenarios (not oracle fixtures: no committed trace).
pub const PERF_DIR: &str = "fixtures/perf/olx";
/// The busiest oracle scenarios (by trace size), timed as they are.
pub const TIMED: [&str; 6] = [
    "two-worms-fire",
    "multi-shot",
    "suicide-score",
    "fire-minigun",
    "weapons/classic/spikeballs",
    "weapons/classic/uzi",
];
/// The heavy ones, in [`PERF_DIR`].
pub const HEAVY: [&str; 2] = ["heavy-spikeballs", "heavy-napalm"];

/// One frame OLX timed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OlxFrame {
    pub frame: i64,
    pub ns: u64,
    pub projectiles: u32,
    pub rand_calls: u64,
}

/// `frame ns projectiles rc` lines.
pub fn parse_timing(text: &str) -> Result<Vec<OlxFrame>, String> {
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .enumerate()
        .map(|(n, l)| {
            let bad = || format!("timing line {}: {l:?}", n + 1);
            let f: Vec<&str> = l.split_whitespace().collect();
            if f.len() != 4 {
                return Err(bad());
            }
            Ok(OlxFrame {
                frame: f[0].parse().map_err(|_| bad())?,
                ns: f[1].parse().map_err(|_| bad())?,
                projectiles: f[2].parse().map_err(|_| bad())?,
                rand_calls: f[3].parse().map_err(|_| bad())?,
            })
        })
        .collect()
}

/// Why OLX cannot be timed here, if it cannot: Docker is not installed or
/// its daemon is not running (ruling PF5: `envelope` makes it a notice).
pub fn docker_missing() -> Option<String> {
    let out = std::process::Command::new("docker").args(["info", "--format", "{{.ServerVersion}}"]).output();
    docker_problem(
        out.map(|o| (o.status.success(), String::from_utf8_lossy(&o.stderr).into_owned()))
            .map_err(|e| e.to_string()),
    )
}

/// [`docker_missing`] from `docker info`'s outcome: whether it ran and
/// succeeded, and its stderr.
fn docker_problem(info: Result<(bool, String), String>) -> Option<String> {
    match info {
        Err(e) => Some(format!("Docker is not installed (running `docker`: {e})")),
        Ok((false, stderr)) => Some(format!(
            "Docker is not running (`docker info`: {}); start it with `sudo service docker start`",
            stderr.lines().find(|l| !l.trim().is_empty()).unwrap_or("failed").trim()
        )),
        Ok((true, _)) => None,
    }
}

/// The oracle's OLX image, built if needed, and the user it runs as.
pub struct Olx {
    image: String,
    user: String,
}

impl Olx {
    /// Fails with [`docker_missing`]'s reason before building anything.
    pub fn open() -> anyhow::Result<Olx> {
        if let Some(why) = docker_missing() {
            anyhow::bail!(why);
        }
        Ok(Olx {
            image: oracle::ensure_image(&root())?,
            user: format!("{}:{}", oracle::id("-u")?, oracle::id("-g")?),
        })
    }

    pub fn image(&self) -> &str {
        &self.image
    }

    /// Plays `sc` in OLX with timing on: its frames; the header-only trace
    /// gzipped to `out_gz` when given (for `rl-cli scenario --time`).
    pub fn time(&self, name: &str, sc: &Scenario, out_gz: Option<&Path>) -> anyhow::Result<Vec<OlxFrame>> {
        let work = root().join("target/envelope/olx-work").join(name);
        let env = [("ORACLE_TIMING", "/work/timing.txt"), ("ORACLE_HEADER_ONLY", "1")];
        let trace = oracle::record_once_with(&self.image, &self.user, &work, sc, &env)
            .with_context(|| name.to_string())?;
        if let Some(gz) = out_gz {
            if let Some(dir) = gz.parent() {
                std::fs::create_dir_all(dir)?;
            }
            std::fs::write(gz, rl_oracle::gzip(&trace))?;
        }
        let text = std::fs::read_to_string(work.join("timing.txt")).context("OLX wrote no timing file")?;
        let frames = parse_timing(&text).map_err(anyhow::Error::msg)?;
        anyhow::ensure!(
            frames.len() == sc.frames as usize,
            "{name}: OLX timed {} frames of {}",
            frames.len(),
            sc.frames
        );
        anyhow::ensure!(
            frames.iter().zip(0..).all(|(f, n)| f.frame == n),
            "{name}: OLX's timed frames are not 0, 1, 2, ... in order"
        );
        Ok(frames)
    }

    /// How OLX was built: the image's release compiler flags and compiler,
    /// such as `CMAKE_CXX_FLAGS_RELEASE:STRING=-O3 -DNDEBUG; c++ (Ubuntu
    /// 11.4.0-1ubuntu1~22.04.3) 11.4.0`.
    ///
    /// The build is Release (ruling T6-A): the Dockerfile passes `-DDEBUG=No`,
    /// so `CMakeOlxCommon.cmake:43-47` sets `CMAKE_BUILD_TYPE Release` as a
    /// plain variable, and line 369 (338 on MinGW) adds `-O3`. The cache's
    /// `CMAKE_BUILD_TYPE` entry stays empty, so it is never read here: an
    /// empty value must not be reported as the build type.
    pub fn build_info(&self) -> String {
        let script = "grep -m1 '^CMAKE_CXX_FLAGS_RELEASE:' /olx/CMakeCache.txt; c++ --version | head -1";
        std::process::Command::new("docker")
            .args(["run", "--rm", &self.image, "sh", "-c", script])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().replace('\n', "; "))
            .unwrap_or_else(|e| format!("unknown ({e})"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timing_lines_parse() {
        let f = parse_timing("0 1500 2 17\n1 2500 4 30\n").unwrap();
        assert_eq!(f[1], OlxFrame { frame: 1, ns: 2500, projectiles: 4, rand_calls: 30 });
        assert!(parse_timing("0 1500 2\n").is_err());
        assert!(parse_timing("x 1 2 3\n").is_err());
        assert_eq!(parse_timing("\n").unwrap(), []);
    }

    /// Ruling PF5: Docker missing or stopped is named, for a notice.
    #[test]
    fn docker_problems_are_named() {
        assert_eq!(docker_problem(Ok((true, String::new()))), None);
        let stopped =
            docker_problem(Ok((false, "\nCannot connect to the Docker daemon\nmore\n".into()))).unwrap();
        assert!(
            stopped.contains("not running") && stopped.contains("Cannot connect to the Docker daemon"),
            "{stopped}"
        );
        assert!(!stopped.contains("more"), "{stopped}");
        let absent = docker_problem(Err("No such file or directory".into())).unwrap();
        assert!(absent.contains("not installed") && absent.contains("No such file"), "{absent}");
    }

    #[test]
    fn the_timed_scenarios_exist() {
        for name in TIMED {
            assert!(root().join("fixtures/oracle").join(format!("{name}.ron")).is_file(), "{name}");
        }
        for name in HEAVY {
            let p = root().join(PERF_DIR).join(format!("{name}.ron"));
            let sc = Scenario::from_ron(&std::fs::read_to_string(&p).unwrap()).unwrap();
            assert!(sc.worms.len() >= 8 && sc.frames >= 500, "{name}");
            assert_eq!(sc.settings.get("GameOptions.GameInfo.LoadingTime").map(String::as_str), Some("0"));
        }
    }
}
