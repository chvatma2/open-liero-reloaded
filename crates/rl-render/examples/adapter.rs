//! Prints the adapter offscreen renders use, or exits with status 3 when
//! none opens (`cargo xtask full` then skips the GPU tests with a notice).

fn main() -> std::process::ExitCode {
    match rl_render::Offscreen::new() {
        Ok(gpu) => {
            let i = gpu.adapter_info();
            println!("{} ({:?}, {:?})", i.name, i.backend, i.device_type);
            std::process::ExitCode::SUCCESS
        }
        Err(e) => {
            println!("{e}");
            std::process::ExitCode::from(3)
        }
    }
}
