//! The level editor in the shell (plan 14c Task 6): the Editor screen's
//! commands open an `EditorHost`, which replaces the menus until it
//! closes; its Play runs as an edited game (`Launch::Edited`: no replay,
//! Esc or Stop returns), and closing the window asks first when there are
//! unsaved changes (Review Focus 1).

use std::path::{Path, PathBuf};
use std::time::Instant;

use olr_editor::EditorAction;
use olr_ui::{AppCommand, Screen, UiAction};

use super::Shell;
use crate::editor::{self, CloseDecision, EditorHost, HostOut};

impl Shell {
    /// Whether the editor is open (its panels and canvas replace the menus).
    pub fn editor_open(&self) -> bool {
        self.editor.is_some()
    }

    pub fn editor(&mut self) -> Option<&mut EditorHost> {
        self.editor.as_mut()
    }

    /// Whether the editor's Play runs.
    pub fn editing_play(&self) -> bool {
        self.editor.is_some() && self.play.is_some()
    }

    /// The Editor screen's commands (`AppCommand::{EditLevel, CopyLevel,
    /// NewLevel, Recover, DiscardRecovery}`).
    pub(super) fn editor_command(&mut self, c: AppCommand) {
        if let AppCommand::DiscardRecovery(id) = &c {
            if let Some(dir) = self.store.recovery_dir()
                && let Err(e) = editor::discard_recovery(&dir, id)
            {
                self.data.editor_message = Some(format!("{e:#}"));
            }
            self.refresh_editor_packs();
            return;
        }
        let roots = self.roots();
        let roots: Vec<&Path> = roots.iter().map(PathBuf::as_path).collect();
        let mut host = match EditorHost::new(&mut self.store, &roots) {
            Ok(h) => h,
            Err(e) => {
                self.data.editor_message = Some(format!("{e:#}"));
                return;
            }
        };
        let out = host.open_from_menu(&c);
        self.log(host.take_warnings());
        self.adopt(host, out);
    }

    /// `olr --edit <dir>` (R6 checked by the caller): the editor on that
    /// pack before the first frame, or why it cannot open.
    pub fn open_editor_dir(&mut self, dir: &Path) -> Result<(), String> {
        let roots = self.roots();
        let roots: Vec<&Path> = roots.iter().map(PathBuf::as_path).collect();
        let mut host = EditorHost::new(&mut self.store, &roots).map_err(|e| format!("{e:#}"))?;
        let out = host.open_dir(dir);
        self.log(host.take_warnings());
        self.adopt(host, out);
        match self.data.editor_message.take() {
            Some(why) if self.editor.is_none() => Err(why),
            _ => Ok(()),
        }
    }

    /// Keeps `host` when it opened a document (its `out` waits for the
    /// app's next editor frame), else says why on the Editor screen.
    fn adopt(&mut self, host: EditorHost, out: Vec<HostOut>) {
        if host.state().document().is_none() {
            let why = out.iter().find_map(|o| match o {
                HostOut::Message(m) => Some(m.clone()),
                _ => None,
            });
            self.data.editor_message = Some(why.unwrap_or_else(|| "the level did not open".into()));
            // A pack may have been made (a copy, a new level) all the same.
            self.refresh_editor_packs();
            return;
        }
        self.data.editor_message = None;
        self.editor = Some(host);
        self.editor_out.extend(out);
    }

    /// One frame of the editor: `actions` from `olr_editor::ui::show` and
    /// the recovery timer at `now`. Play, Closed and messages are the
    /// shell's; the canvas's uploads (`Upload`, `FullUpload`, also those
    /// waiting since the editor opened) are returned for the app.
    pub fn editor_frame(&mut self, actions: Vec<EditorAction>, now: Instant) -> Vec<HostOut> {
        let Some(host) = &mut self.editor else { return Vec::new() };
        let mut out = std::mem::take(&mut self.editor_out);
        out.extend(host.act(actions));
        out.extend(host.tick(now));
        let mut canvas = Vec::new();
        for o in out {
            match o {
                HostOut::Upload(_) | HostOut::FullUpload => canvas.push(o),
                HostOut::Play(p) => self.play_edited(*p),
                HostOut::Closed => self.close_editor(),
                HostOut::Message(m) => {
                    eprintln!("warning: editor: {m}");
                    if let Some(h) = &mut self.editor {
                        h.act(vec![EditorAction::Note(m)]);
                    }
                }
            }
        }
        canvas
    }

    /// F5 in the editor's Play: the level again from its start.
    pub fn restart_edited(&mut self) {
        if self.editing_play() {
            self.restart();
        }
    }

    /// The editor's Play ends (Esc, Stop, or the load failed): back to the
    /// editor exactly as it was; the match is thrown away.
    pub fn stop_edited(&mut self) {
        if self.play.is_some() {
            self.end();
        }
        if let Some(h) = &mut self.editor {
            h.act(vec![EditorAction::PlayEnded]);
        }
    }

    /// The editor closed: the menus come back on the Editor screen, the
    /// content listed again (a new or copied pack shows).
    pub fn close_editor(&mut self) {
        self.editor = None;
        self.editor_out.clear();
        self.list_content();
        self.refresh_editor_packs();
        if self.ui.screen() != Screen::Editor {
            self.ui.update(UiAction::Open(Screen::Editor));
        }
    }

    /// The data directory's level packs and recovery copies, for the
    /// Editor screen.
    pub fn refresh_editor_packs(&mut self) {
        let editable = self.packs_dir.as_deref().map(editor::editable_levels).unwrap_or_default();
        let recoveries =
            self.store.recovery_dir().as_deref().map(editor::list_recoveries).unwrap_or_default();
        self.data.set_editor_packs(editable, recoveries);
    }

    /// The window's close button: with the editor open, its Close first
    /// (ending its Play), so unsaved changes are asked about (Review Focus 1).
    pub fn request_close(&mut self) -> CloseDecision {
        if self.editing_play() {
            self.end();
        }
        let d = editor::on_close_request(self.editor.as_mut());
        if d == CloseDecision::Quit && self.editor.is_some() {
            self.editor = None;
        }
        d
    }
}
