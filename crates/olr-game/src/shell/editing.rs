//! The level editor in the shell (plan 14c Task 6): the Editor screen's
//! commands open an `EditorHost`, which replaces the menus until it
//! closes; its Play runs as an edited game (`Launch::Edited`: no replay,
//! Esc or Stop returns), and closing the window asks first when there are
//! unsaved changes (Review Focus 1). The skin editor (issue #14) likewise:
//! the Skin tab's commands open a `SkinHost`; each save lists the content
//! again, so the profiles' skin picker offers the skin at once.

use std::path::{Path, PathBuf};
use std::time::Instant;

use olr_editor::EditorAction;
use olr_ui::{AppCommand, Screen, UiAction};

use super::Shell;
use crate::editor::skin::{SkinHost, SkinOut};
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
    /// shell's; the canvas's uploads (`Upload`, `FullUpload`, `Distant`, also those
    /// waiting since the editor opened) are returned for the app.
    pub fn editor_frame(&mut self, actions: Vec<EditorAction>, now: Instant) -> Vec<HostOut> {
        let Some(host) = &mut self.editor else { return Vec::new() };
        let mut out = std::mem::take(&mut self.editor_out);
        out.extend(host.act(actions));
        out.extend(host.tick(now));
        let mut canvas = Vec::new();
        for o in out {
            match o {
                HostOut::Upload(_) | HostOut::FullUpload | HostOut::Distant(_) => canvas.push(o),
                HostOut::Play(p) => self.play_edited(*p),
                HostOut::Closed => self.close_editor(),
                HostOut::Message(m) => {
                    log::warn!("editor: {m}");
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

    /// The data directory's level and skin packs and recovery copies, for
    /// the Editor screen.
    pub fn refresh_editor_packs(&mut self) {
        let editable = self.packs_dir.as_deref().map(editor::editable_levels).unwrap_or_default();
        let recoveries =
            self.store.recovery_dir().as_deref().map(editor::list_recoveries).unwrap_or_default();
        self.data.set_editor_packs(editable, recoveries);
        let skins = self.packs_dir.as_deref().map(editor::editable_skins).unwrap_or_default();
        self.data.set_editable_skins(skins);
    }

    /// Whether the skin editor is open (its panels replace the menus).
    pub fn skin_editor_open(&self) -> bool {
        self.skin_editor.is_some()
    }

    pub fn skin_editor(&mut self) -> Option<&mut SkinHost> {
        self.skin_editor.as_mut()
    }

    /// The Skin tab's commands (`AppCommand::{EditSkin, NewSkin}`): the
    /// skin editor opens, or the Editor screen says why not.
    pub(super) fn skin_command(&mut self, c: AppCommand) {
        let Some(packs) = self.packs_dir.clone() else {
            self.data.editor_message = Some("there is no data directory to save skins in".into());
            return;
        };
        let bundled = olr_content::bundled_roots(&self.cfg.content);
        match SkinHost::open(&c, &packs, &bundled, self.content.as_ref()) {
            Ok(h) => {
                self.data.editor_message = None;
                self.skin_editor = Some(h);
            }
            Err(why) => {
                log::warn!("skin editor: {why}");
                self.data.editor_message = Some(why);
            }
        }
    }

    /// One frame of the skin editor: `actions` from
    /// `olr_skin_editor::ui::show` and the preview's clock at `now`. A save
    /// lists the content again (the skin picker offers it at once); Close
    /// brings the Editor screen back.
    pub fn skin_frame(&mut self, actions: Vec<olr_skin_editor::SkinAction>, now: Instant) {
        let Some(host) = &mut self.skin_editor else { return };
        let out = host.act(actions);
        host.tick(now);
        if out.contains(&SkinOut::Saved) {
            self.list_content();
            self.refresh_editor_packs();
        }
        if out.contains(&SkinOut::Closed) {
            self.close_skin_editor();
        }
    }

    /// The skin editor closed: the menus come back on the Editor screen.
    pub fn close_skin_editor(&mut self) {
        self.skin_editor = None;
        self.refresh_editor_packs();
        if self.ui.screen() != Screen::Editor {
            self.ui.update(UiAction::Open(Screen::Editor));
        }
    }

    /// The window's close button: with the editor open, its Close first
    /// (ending its Play), so unsaved changes are asked about (Review Focus 1).
    pub fn request_close(&mut self) -> CloseDecision {
        if self.editing_play() {
            self.end();
        }
        if let Some(h) = &mut self.skin_editor {
            if h.act(vec![olr_skin_editor::SkinAction::Close]).contains(&SkinOut::Closed) {
                self.skin_editor = None;
            } else {
                return CloseDecision::Wait;
            }
        }
        let d = editor::on_close_request(self.editor.as_mut());
        if d == CloseDecision::Quit && self.editor.is_some() {
            self.editor = None;
        }
        d
    }
}
