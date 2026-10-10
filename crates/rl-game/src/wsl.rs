//! WSL2's graphics defaults (plan 5 Global Constraints): wgpu's GL backend,
//! and Mesa's d3d12 Gallium driver for the real GPU, unless the user chose.
//!
//! Mesa reads `GALLIUM_DRIVER` when the driver loads, and setting an
//! environment variable in a running process is `unsafe` (this workspace
//! forbids it), so the game starts itself again with the variable set.

/// Whether `osrelease` (`/proc/sys/kernel/osrelease`) is a WSL kernel's.
pub fn is_wsl(osrelease: &str) -> bool {
    osrelease.to_ascii_lowercase().contains("microsoft")
}

/// The backends to ask wgpu for: `WGPU_BACKEND` if set, else GL on WSL
/// (Vulkan there finds only llvmpipe, which fails on Wayland), else all.
pub fn backends(wsl: bool, wgpu_backend: Option<&str>) -> wgpu::Backends {
    match wgpu_backend {
        Some(b) => wgpu::Backends::from_comma_list(b),
        None if wsl => wgpu::Backends::GL,
        None => wgpu::Backends::all(),
    }
}

/// The environment to start the game again with, if it must: on WSL when
/// the user set neither `WGPU_BACKEND` nor `GALLIUM_DRIVER`.
pub fn relaunch_env(
    wsl: bool,
    wgpu_backend: Option<&str>,
    gallium: Option<&str>,
) -> Option<[(&'static str, &'static str); 2]> {
    (wsl && wgpu_backend.is_none() && gallium.is_none())
        .then_some([("GALLIUM_DRIVER", "d3d12"), ("WGPU_BACKEND", "gl")])
}

/// Reads the kernel's release string; empty off Linux.
pub fn osrelease() -> String {
    std::fs::read_to_string("/proc/sys/kernel/osrelease").unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wsl_defaults_to_gl_on_d3d12_unless_the_user_chose() {
        assert!(is_wsl("6.6.87.2-microsoft-standard-WSL2\n"));
        assert!(!is_wsl("6.8.0-45-generic"));
        assert_eq!(backends(true, None), wgpu::Backends::GL);
        assert_eq!(backends(false, None), wgpu::Backends::all());
        assert_eq!(backends(true, Some("vulkan")), wgpu::Backends::VULKAN);
        assert_eq!(relaunch_env(true, None, None).unwrap()[0], ("GALLIUM_DRIVER", "d3d12"));
        assert_eq!(relaunch_env(true, Some("gl"), None), None);
        assert_eq!(relaunch_env(true, None, Some("llvmpipe")), None);
        assert_eq!(relaunch_env(false, None, None), None);
    }
}
