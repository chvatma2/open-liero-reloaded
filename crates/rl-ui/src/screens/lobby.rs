//! The Lobby screen (plan 10 task 11, spec §7.2): on the left the match's
//! setup, the Local Play panels (editable for the host, read-only and
//! greyed for the others) and this player's weapons; on the right the
//! members (name, key fingerprint, colour, seat, ready, ping, missing
//! content or its download's percent, and the host's kick, ban and move
//! buttons), the content downloads (plan 11: name, kind, progress,
//! state) and the chat; below, Start and "Force start" for the host, else
//! "Waiting for the host". The host's invite code (Copy) and, for a
//! public game, whether it is listed on the Internet, over the setup
//! (plan 12a); under them its reachability's lines (plan 12b: its
//! router's port, its relays), and on a client how it reaches its host;
//! each member's path tag beside its ping. Ready is disabled while this player lacks
//! required content. Content over the prompt asks "Download" or "Leave"
//! first. Back leaves the lobby, after a confirmation when a match runs.

use egui::{Button, Color32, Key, ProgressBar, RichText, ScrollArea, Sense, TextEdit, Ui, vec2};

use super::local_play::{level_combo, level_grid, mod_picker, olx_match, personality_combo, rules};
use super::{MenuData, columns, footer, message, page, primary, section, tall_section};
use crate::hud::LineKind;
use crate::match_prefs::{MAX_BOTS, MatchPrefs};
use crate::online::{CHAT_CHARS, DownloadView, LobbyView, MemberView, PHASE_LOBBY, megabytes};
use crate::state::{Screen, UiAction, UiState};
use crate::theme;

const WIDTH: f32 = 1100.0;
const SECTION_GAP: f32 = 8.0;
const RED: Color32 = Color32::from_rgb(255, 110, 90);

/// What the Force start button says, enabled.
pub const FORCE_HINT: &str = "Players who are not ready, or lack the content, watch this match.";
/// Why Ready is disabled.
pub const READY_MISSING: &str = "get the missing content first";

/// The screen's own state.
#[derive(Default)]
pub(super) struct View {
    /// The chat's input line.
    chat: String,
    /// "Leave the match?" is asking.
    confirm_leave: bool,
    /// "Ban <name>?" is asking: the member's id and name.
    confirm_ban: Option<(u8, String)>,
}

/// What the lobby's Back does: leaves, unless a match runs (then it asks,
/// and a second Back cancels the question). Called by `screens::show` on
/// every Back of the lobby screen, which it replaces.
pub(super) fn intercept_back(data: &mut MenuData, out: &mut Vec<UiAction>) {
    let running = data.lobby.as_ref().is_some_and(|l| l.phase != PHASE_LOBBY);
    let (confirm, ban) = (&mut data.lobby_ui.confirm_leave, &mut data.lobby_ui.confirm_ban);
    out.retain(|a| {
        if *a != UiAction::Back {
            return true;
        }
        if ban.take().is_some() {
            false
        } else if *confirm {
            *confirm = false;
            false
        } else if running {
            *confirm = true;
            false
        } else {
            true
        }
    });
}

/// Whether the host may force a start: two worms (players and bots) and a
/// member to play, whatever their readiness and content.
fn can_force(v: &LobbyView) -> bool {
    let players = v.members.iter().filter(|m| !m.is_spectator()).count();
    v.phase == PHASE_LOBBY && !v.members.is_empty() && players + usize::from(v.setup.bots) >= 2
}

pub(super) fn show(ui: &mut Ui, state: &UiState, data: &mut MenuData, out: &mut Vec<UiAction>) {
    let view = data.lobby.clone();
    let mut ui = page(ui, "Lobby", WIDTH, out);
    let ui = &mut ui;
    // The match ended: the question is moot.
    if view.as_ref().is_some_and(|v| v.phase == PHASE_LOBBY) {
        data.lobby_ui.confirm_leave = false;
    }
    let Some(view) = view else {
        ui.label("Connecting…");
        message(ui, state);
        return;
    };
    let mut ui = footer(ui, "lobby", |ui| {
        if view.host {
            ui.horizontal(|ui| {
                let startable = view.phase == PHASE_LOBBY && view.can_start.is_ok();
                let start = primary(ui, "Start", startable);
                let start = match &view.can_start {
                    Err(why) => start.on_disabled_hover_text(why),
                    Ok(()) => start,
                };
                if start.clicked() {
                    out.push(UiAction::LobbyStart);
                }
                let force =
                    ui.add_enabled(can_force(&view), Button::new("Force start").min_size(vec2(110.0, 44.0)));
                let force = force
                    .on_hover_text(FORCE_HINT)
                    .on_disabled_hover_text("Needs at least two players or bots");
                if force.clicked() {
                    out.push(UiAction::LobbyForceStart);
                }
            });
        } else if view.phase == PHASE_LOBBY {
            ui.label(RichText::new("Waiting for the host").size(18.0));
        } else {
            ui.label(RichText::new(&view.phase).size(18.0));
        }
        if let Some(s) = &data.online_status {
            ui.label(RichText::new(s).color(Color32::from_gray(190)));
        }
        message(ui, state);
    });
    let ui = &mut ui;
    columns(ui, 2, |c| {
        setup(&mut c[0], &view, data, out);
        members_and_chat(&mut c[1], &view, data, out);
    });
    if data.lobby_ui.confirm_leave {
        leave_dialog(ui.ctx(), &mut data.lobby_ui, out);
    }
    if data.lobby_ui.confirm_ban.is_some() {
        ban_dialog(ui.ctx(), &mut data.lobby_ui, out);
    }
    if let Some(bytes) = view.prompt {
        download_prompt(ui.ctx(), bytes, out);
    }
}

/// The content over the prompt (plan 11): download it, or leave.
fn download_prompt(ctx: &egui::Context, bytes: u64, out: &mut Vec<UiAction>) {
    egui::Window::new("Download content?")
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, vec2(0.0, 0.0))
        .show(ctx, |ui| {
            ui.label(format!("This game needs {} of content from the host.", megabytes(bytes)));
            ui.horizontal(|ui| {
                if ui.button("Download").clicked() {
                    out.push(UiAction::LobbyDownload(true));
                }
                if ui.button("Leave").clicked() {
                    out.push(UiAction::LobbyDownload(false));
                }
            });
        });
}

fn ban_dialog(ctx: &egui::Context, v: &mut View, out: &mut Vec<UiAction>) {
    let Some((member, name)) = v.confirm_ban.clone() else { return };
    egui::Window::new(format!("Ban {name}?"))
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, vec2(0.0, 0.0))
        .show(ctx, |ui| {
            ui.label("They cannot join again with this key.");
            ui.horizontal(|ui| {
                if ui.button("Yes, ban").clicked() {
                    v.confirm_ban = None;
                    out.push(UiAction::LobbyKick { member, ban: true });
                }
                if ui.button("Cancel").clicked() {
                    v.confirm_ban = None;
                }
            });
        });
}

fn leave_dialog(ctx: &egui::Context, v: &mut View, out: &mut Vec<UiAction>) {
    egui::Window::new("Leave the match?")
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, vec2(0.0, 0.0))
        .show(ctx, |ui| {
            ui.label("A match is running in this lobby.");
            ui.horizontal(|ui| {
                if ui.button("Leave").clicked() {
                    v.confirm_leave = false;
                    out.push(UiAction::LeaveLobby);
                }
                if ui.button("Stay").clicked() {
                    v.confirm_leave = false;
                }
            });
        });
}

/// The left column: the setup (the host's), and this player's weapons.
fn setup(ui: &mut Ui, view: &LobbyView, data: &mut MenuData, out: &mut Vec<UiAction>) {
    let before = view.setup.clone();
    let mut p = before.clone();
    ScrollArea::vertical().id_salt("lobby setup").auto_shrink([false, false]).show(ui, |ui| {
        if view.invite.is_some() || view.listing.is_some() || !view.reach.is_empty() {
            section(ui, "Invite", |ui, _| invite(ui, view));
            ui.add_space(SECTION_GAP);
        }
        if let Some(c) = &view.connection {
            section(ui, "Connection", |ui, _| {
                ui.label(RichText::new(c).color(Color32::from_gray(190)));
            });
            ui.add_space(SECTION_GAP);
        }
        section(ui, "Your weapons", |ui, _| {
            ui.horizontal(|ui| {
                if ui.button("Random").clicked() {
                    out.push(UiAction::LobbyLoadout(None));
                }
                if ui.add_enabled(data.weapons.is_some(), Button::new("Choose weapons…")).clicked() {
                    out.push(UiAction::Open(Screen::WeaponSelect));
                }
            });
        });
        ui.add_space(SECTION_GAP);
        ui.add_enabled_ui(view.host, |ui| {
            section(ui, "Map", |ui, _| {
                mod_picker(ui, data, &mut p);
                ui.add_space(8.0);
                ScrollArea::vertical().id_salt("lobby levels").max_height(240.0).show(ui, |ui| {
                    level_grid(ui, data, &mut p);
                });
            });
            ui.add_space(SECTION_GAP);
            section(ui, "Match (Deathmatch)", |ui, _| {
                if rules(ui, data, &mut p) {
                    p = olx_match(data, &p);
                }
            });
            ui.add_space(SECTION_GAP);
            section(ui, "Bots", |ui, _| bots(ui, &mut p));
        });
    });
    if view.host && p != before {
        out.push(UiAction::LobbySetup(p));
    }
}

/// The hosted game's invite code with its Copy (egui's clipboard),
/// whether a public game is listed on the Internet, and how players reach
/// it (each line wraps in the column).
fn invite(ui: &mut Ui, view: &LobbyView) {
    if let Some(code) = &view.invite {
        ui.horizontal(|ui| {
            if ui.button("Copy").on_hover_text("Puts the invite code on the clipboard").clicked() {
                ui.ctx().copy_text(code.clone());
            }
            ui.add(egui::Label::new(RichText::new(code).monospace().size(12.0)).wrap());
        });
    }
    if let Some(listing) = &view.listing {
        ui.label(RichText::new(listing).weak());
    }
    for line in &view.reach {
        ui.label(RichText::new(line).color(Color32::from_gray(190)));
    }
}

/// The bots of a lobby: none to [`MAX_BOTS`] (a lobby of people needs
/// none), one difficulty for all.
fn bots(ui: &mut Ui, p: &mut MatchPrefs) {
    ui.horizontal(|ui| {
        ui.label(format!("{} bots", p.bots));
        if ui.add_enabled(p.bots < MAX_BOTS, Button::new("Add bot")).clicked() {
            p.set_bots(p.bots + 1);
        }
        if ui.add_enabled(p.bots > 0, Button::new("Remove bot")).clicked() {
            p.bots -= 1;
            let n = usize::from(p.bots);
            if let Some(levels) = &mut p.per_bot {
                levels.truncate(n);
            }
        }
    });
    if p.bots > 0 {
        ui.horizontal(|ui| {
            let label = ui.label("Difficulty");
            level_combo(ui, label.id, "lobby difficulty", &mut p.difficulty);
        });
        ui.horizontal(|ui| {
            let label = ui.label("Personality");
            personality_combo(ui, label.id, "lobby personality", &mut p.personality);
        });
    }
}

fn members_and_chat(ui: &mut Ui, view: &LobbyView, data: &mut MenuData, out: &mut Vec<UiAction>) {
    section(ui, "Players", |ui, _| {
        ScrollArea::vertical().id_salt("lobby members").max_height(250.0).auto_shrink([false, true]).show(
            ui,
            |ui| {
                for m in &view.members {
                    member(ui, view, m, &mut data.lobby_ui, out);
                }
            },
        );
        ui.add_space(4.0);
        let me = view.members.iter().find(|m| m.id == view.me);
        ui.horizontal(|ui| {
            let mut ready = me.is_some_and(|m| m.ready);
            let lacking = me.is_some_and(|m| !m.missing.is_empty());
            let r = ui.add_enabled(!lacking, egui::Checkbox::new(&mut ready, "Ready"));
            if r.on_disabled_hover_text(READY_MISSING).changed() {
                out.push(UiAction::LobbyReady(ready));
            }
            let mut spectate = me.is_some_and(MemberView::is_spectator);
            if ui.checkbox(&mut spectate, "Spectate").changed() {
                out.push(UiAction::LobbySpectate(spectate));
            }
        });
    });
    if !view.downloads.is_empty() {
        ui.add_space(SECTION_GAP);
        section(ui, "Downloads", |ui, _| {
            ScrollArea::vertical()
                .id_salt("lobby downloads")
                .max_height(120.0)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    for d in &view.downloads {
                        download(ui, d);
                    }
                });
        });
    }
    ui.add_space(SECTION_GAP);
    tall_section(ui, "Chat", |ui, title| {
        let h = (ui.available_height() - 36.0).max(40.0);
        ScrollArea::vertical()
            .id_salt("lobby chat")
            .max_height(h)
            .auto_shrink([false, false])
            .stick_to_bottom(true)
            .show(ui, |ui| {
                for l in &data.chat {
                    let colour = match l.kind {
                        LineKind::Chat => Color32::from_rgb(255, 230, 140),
                        LineKind::Event => Color32::from_gray(170),
                    };
                    ui.label(RichText::new(&l.text).color(colour));
                }
            });
        ui.horizontal(|ui| {
            let v = &mut data.lobby_ui;
            let w = (ui.available_width() - 70.0).max(40.0);
            let field = TextEdit::singleline(&mut v.chat)
                .char_limit(CHAT_CHARS)
                .hint_text("Say something")
                .desired_width(w);
            let r = ui.add(field).labelled_by(title);
            let enter = r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
            let send = ui.add_enabled(!v.chat.trim().is_empty(), Button::new("Send")).clicked();
            if (enter || send) && !v.chat.trim().is_empty() {
                out.push(UiAction::LobbyChat(std::mem::take(&mut v.chat)));
                r.request_focus();
            }
        });
    });
}

fn member(ui: &mut Ui, view: &LobbyView, m: &MemberView, v: &mut View, out: &mut Vec<UiAction>) {
    ui.push_id(("member", m.id), |ui| {
        ui.horizontal_wrapped(|ui| {
            let (rect, _) = ui.allocate_exact_size(vec2(14.0, 14.0), Sense::hover());
            let [r, g, b] = m.colour;
            ui.painter().rect_filled(rect, 3.0, Color32::from_rgb(r, g, b));
            let name = RichText::new(&m.name);
            ui.label(if m.id == view.me { name.strong() } else { name });
            if m.admin {
                ui.label(RichText::new("admin").small().color(theme::ACCENT));
            }
            let fp: String = m.fingerprint.chars().take(8).collect();
            ui.label(RichText::new(fp).monospace().small().color(Color32::from_gray(130)));
            ui.label(
                RichText::new(m.ping_ms.map_or_else(|| "- ms".to_string(), |p| format!("{p} ms")))
                    .small()
                    .weak(),
            );
            if let Some(path) = &m.path {
                ui.label(RichText::new(path).small().color(theme::ACCENT))
                    .on_hover_text("How the host reaches this player");
            }
            ui.label(RichText::new(&m.seat).small());
            ui.label(if m.ready {
                RichText::new("✔ ready").color(Color32::from_rgb(120, 220, 120))
            } else {
                RichText::new("not ready").weak()
            });
            if let Some(pct) = m.download {
                ui.label(RichText::new(format!("downloading {pct}%")).color(theme::ACCENT));
            } else if !m.missing.is_empty() {
                ui.label(RichText::new(format!("missing: {}", m.missing.join(", "))).color(RED));
            }
        });
        if view.host && m.id != view.me {
            ui.horizontal(|ui| {
                if ui.small_button("Kick").clicked() {
                    out.push(UiAction::LobbyKick { member: m.id, ban: false });
                }
                if ui.small_button("Ban").clicked() {
                    v.confirm_ban = Some((m.id, m.name.clone()));
                }
                let (text, spectator) =
                    if m.is_spectator() { ("To players", false) } else { ("To spectators", true) };
                if ui.small_button(text).clicked() {
                    out.push(UiAction::LobbyMove { member: m.id, spectator });
                }
            });
        }
        ui.add_space(4.0);
    });
}

/// One download's row: name, kind, a progress bar and the state.
fn download(ui: &mut Ui, d: &DownloadView) {
    ui.horizontal(|ui| {
        ui.label(&d.name);
        ui.label(RichText::new(&d.kind).small().weak());
        let done = if d.size == 0 { 0.0 } else { (d.have as f64 / d.size as f64).clamp(0.0, 1.0) as f32 };
        ui.add(ProgressBar::new(done).desired_width(120.0).show_percentage());
        let colour = if d.state.starts_with("failed") || d.state == "not available" {
            RED
        } else {
            Color32::from_gray(190)
        };
        ui.label(RichText::new(&d.state).color(colour));
    });
}
