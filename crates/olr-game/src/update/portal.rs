//! The Flatpak portal (`org.freedesktop.portal.Flatpak`, issue #4) over
//! the session bus: `CreateUpdateMonitor`, then the monitor's `Update`
//! with its `Progress` signals, and `Spawn` with
//! `FLATPAK_SPAWN_FLAGS_LATEST_VERSION` to restart into the updated build
//! (the running sandbox keeps the old `/app`). The portal shows no dialog
//! of its own: the Update screen's button is the player's confirmation.
//! The game needs no permission for it (every Flatpak may talk to the
//! portal). Linux only.

use std::collections::HashMap;

use olr_update::flatpak::PortalProgress;
use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};

const BUS: &str = "org.freedesktop.portal.Flatpak";
const PATH: &str = "/org/freedesktop/portal/Flatpak";
const INTERFACE: &str = "org.freedesktop.portal.Flatpak";
const MONITOR: &str = "org.freedesktop.portal.Flatpak.UpdateMonitor";
/// `FLATPAK_SPAWN_FLAGS_LATEST_VERSION`: the newest installed version.
const SPAWN_LATEST_VERSION: u32 = 2;
/// How long the update may go without a `Progress` signal before it is
/// given up (the portal restarted, the monitor gone): it signals as it
/// downloads, so silence this long is a stall.
const STALL: std::time::Duration = std::time::Duration::from_secs(10 * 60);

/// The real portal on the session bus.
pub struct Dbus;

fn proxy<'a>(conn: &Connection, path: &'a str, interface: &'a str) -> zbus::Result<Proxy<'a>> {
    zbus::blocking::proxy::Builder::<Proxy>::new(conn)
        .destination(BUS)?
        .path(path)?
        .interface(interface)?
        .cache_properties(zbus::proxy::CacheProperties::No)
        .build()
}

impl super::Portal for Dbus {
    fn update(&mut self, progress: &mut dyn FnMut(&PortalProgress)) -> Result<(), String> {
        let conn = Connection::session().map_err(|e| format!("no session bus: {e}"))?;
        let portal = proxy(&conn, PATH, INTERFACE).map_err(|e| e.to_string())?;
        let options: HashMap<&str, Value> = HashMap::new();
        let path: OwnedObjectPath = portal
            .call("CreateUpdateMonitor", &(options,))
            .map_err(|e| format!("CreateUpdateMonitor: {e}"))?;
        let monitor = proxy(&conn, path.as_str(), MONITOR).map_err(|e| e.to_string())?;
        // Listening before Update: no signal is missed.
        let signals = monitor.receive_signal("Progress").map_err(|e| e.to_string())?;
        let options: HashMap<&str, Value> = HashMap::new();
        let called: zbus::Result<()> = monitor.call("Update", &("", options));
        if let Err(e) = called {
            let _: zbus::Result<()> = monitor.call("Close", &());
            return Err(format!("Update: {e}"));
        }
        // Read on a thread of its own, so a portal that goes quiet is a
        // timeout here (that thread then waits on, harmlessly, for good).
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::Builder::new()
            .name("flatpak-portal".into())
            .spawn(move || {
                for message in signals {
                    if let Ok((info,)) = message.body().deserialize::<(HashMap<String, OwnedValue>,)>()
                        && tx.send(info).is_err()
                    {
                        break;
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        let mut outcome = Ok(());
        loop {
            let info = match rx.recv_timeout(STALL) {
                Ok(info) => info,
                Err(e) => {
                    outcome = Err(match e {
                        std::sync::mpsc::RecvTimeoutError::Timeout => {
                            format!("no progress for {} minutes", STALL.as_secs() / 60)
                        }
                        std::sync::mpsc::RecvTimeoutError::Disconnected => "the portal went away".into(),
                    });
                    break;
                }
            };
            let number = |k: &str| info.get(k).and_then(|v| u32::try_from(v).ok()).unwrap_or(0);
            let text = |k: &str| {
                info.get(k)
                    .and_then(|v| v.try_clone().ok())
                    .and_then(|v| String::try_from(v).ok())
                    .unwrap_or_default()
            };
            let p = PortalProgress::of(
                number("status"),
                number("n_ops"),
                number("op"),
                number("progress"),
                &text("error"),
                &text("error_message"),
            );
            progress(&p);
            if !matches!(p, PortalProgress::Running(_)) {
                break;
            }
        }
        let _: zbus::Result<()> = monitor.call("Close", &());
        outcome
    }
}

/// Starts this Flatpak's newest installed version with `args` (`olr`
/// and them) through the portal's `Spawn`.
pub fn spawn_latest(args: &[std::ffi::OsString]) -> Result<(), String> {
    use std::os::unix::ffi::OsStrExt as _;
    // GLib's byte strings end in NUL.
    let bytes = |s: &[u8]| s.iter().copied().chain([0]).collect::<Vec<u8>>();
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    let argv: Vec<Vec<u8>> =
        std::iter::once(bytes(b"olr")).chain(args.iter().map(|a| bytes(a.as_bytes()))).collect();
    let conn = Connection::session().map_err(|e| format!("no session bus: {e}"))?;
    let portal = proxy(&conn, PATH, INTERFACE).map_err(|e| e.to_string())?;
    let fds: HashMap<u32, zbus::zvariant::OwnedFd> = HashMap::new();
    let envs: HashMap<String, String> = HashMap::new();
    let options: HashMap<&str, Value> = HashMap::new();
    let _pid: u32 = portal
        .call("Spawn", &(bytes(cwd.as_os_str().as_bytes()), argv, fds, envs, SPAWN_LATEST_VERSION, options))
        .map_err(|e| format!("Spawn: {e}"))?;
    Ok(())
}
