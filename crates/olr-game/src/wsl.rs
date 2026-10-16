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

/// Mesa's d3d12 driver: the adapter whose name contains this is used.
pub const ADAPTER_VAR: &str = "MESA_D3D12_DEFAULT_ADAPTER_NAME";

/// Where WSL mounts the Windows GPU drivers' Linux libraries.
pub const WSL_LIB: &str = "/usr/lib/wsl/lib";

/// The discrete GPU to ask Mesa's d3d12 driver for, judged from the Windows
/// drivers WSL mounts at `wsl_lib`: NVIDIA's ships `libcuda`. Without a
/// name, Mesa takes the first adapter Windows lists, which on a PC with
/// integrated graphics is often the integrated GPU.
pub fn discrete_adapter(wsl_lib: &std::path::Path) -> Option<&'static str> {
    ["libcuda.so.1", "nvidia-smi"].iter().any(|f| wsl_lib.join(f).exists()).then_some("NVIDIA")
}

/// The environment to start the game again with, if it must: on WSL when
/// the user set neither `WGPU_BACKEND` nor `GALLIUM_DRIVER`. It names the
/// `discrete` adapter for Mesa unless the user chose one (`adapter`, the
/// inherited [`ADAPTER_VAR`]).
pub fn relaunch_env(
    wsl: bool,
    wgpu_backend: Option<&str>,
    gallium: Option<&str>,
    adapter: Option<&str>,
    discrete: Option<&'static str>,
) -> Option<Vec<(&'static str, &'static str)>> {
    if !wsl || wgpu_backend.is_some() || gallium.is_some() {
        return None;
    }
    let mut env = vec![("GALLIUM_DRIVER", "d3d12"), ("WGPU_BACKEND", "gl")];
    if let (None, Some(name)) = (adapter, discrete) {
        env.push((ADAPTER_VAR, name));
    }
    Some(env)
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
        assert_eq!(relaunch_env(true, None, None, None, None).unwrap()[0], ("GALLIUM_DRIVER", "d3d12"));
        assert_eq!(relaunch_env(true, Some("gl"), None, None, None), None);
        assert_eq!(relaunch_env(true, None, Some("llvmpipe"), None, None), None);
        assert_eq!(relaunch_env(false, None, None, None, None), None);
    }

    /// A PC with an integrated and a discrete GPU: Mesa's d3d12 driver takes
    /// the first adapter Windows lists (often the integrated one) unless
    /// told; the game asks for the discrete one, and the user's choice wins.
    #[test]
    fn wsl_prefers_the_discrete_gpu_unless_the_user_chose() {
        let adapter =
            |env: Vec<(&str, &str)>| env.iter().find(|(k, _)| *k == ADAPTER_VAR).map(|(_, v)| v.to_string());
        let env = relaunch_env(true, None, None, None, Some("NVIDIA")).unwrap();
        assert_eq!(adapter(env), Some("NVIDIA".to_string()));
        let env = relaunch_env(true, None, None, Some("AMD"), Some("NVIDIA")).unwrap();
        assert_eq!(adapter(env), None, "the user's own choice is inherited, not overridden");
        let env = relaunch_env(true, None, None, None, None).unwrap();
        assert_eq!(adapter(env), None);
    }

    #[test]
    fn nvidia_is_found_from_wsl_driver_files() {
        let dir = std::env::temp_dir().join(format!("olr-wsl-lib-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(discrete_adapter(&dir), None);
        std::fs::write(dir.join("libcuda.so.1"), b"").unwrap();
        assert_eq!(discrete_adapter(&dir), Some("NVIDIA"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
