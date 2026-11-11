//! What the Awra frontend asks of the app: the parts of a frame that are not
//! drawing. `frame_ui` does the same around egui's views; here the views are
//! Awra widgets that read the app after each turn and queue `Action`s.

use super::App;
use crate::model::Action;

impl App {
    /// One turn of the app for a frontend that draws on its own: events,
    /// timers and queued actions (`background_frame`), then what `frame_ui`
    /// does around its views. `focused` is whether the window has the
    /// keyboard focus.
    pub fn frontend_turn(&mut self, ctx: &egui::Context, focused: bool) {
        self.background_frame(ctx);
        self.settle_pending_reply();
        let locked = self.app_lock.is_locked();
        let focused = focused && !self.window_hidden && !locked;
        let regained_focus = focused && !self.window_focused;
        // Messages received while away are read once the reader is back.
        if regained_focus
            && self.chat_view()
            && let Some(open) = self.open_chat.clone()
            && self.chat(&open).is_some_and(|chat| chat.unread > 0)
        {
            self.mark_read(&open);
        }
        self.window_focused = focused;
        self.report_presence();
        self.apply_actions(ctx);
        if !locked && let Some(wait) = self.watch_link_preview(ctx.input(|input| input.time)) {
            ctx.request_repaint_after(wait);
        }
    }

    /// Starts every account's link and archive, which wait for the first
    /// frame so that the window shows before they load.
    pub fn start_backends(&mut self) {
        for account in &mut self.accounts {
            if let Some(startup) = account.backend.take_startup() {
                let _ = startup.send(());
            }
        }
    }

    /// Saves and stops everything as the window's last close does.
    pub fn shutdown_for_frontend(&mut self) {
        self.shutdown();
    }

    /// The profile picture on disk for `id`, without asking for one.
    /// The views ask at every turn for every row, so a picture looked for on
    /// disk is looked for once: one that arrives later lands in `avatars`.
    pub fn avatar_path(&self, id: &str) -> Option<std::path::PathBuf> {
        if let Some(known) = self.avatars.get(id) {
            return known.clone();
        }
        thread_local! {
            static ON_DISK: std::cell::RefCell<std::collections::HashMap<(String, String), Option<std::path::PathBuf>>> =
                std::cell::RefCell::default();
        }
        let key = (self.account().id.0.clone(), id.to_owned());
        ON_DISK.with(|cache| {
            cache
                .borrow_mut()
                .entry(key)
                .or_insert_with(|| self.cached_avatar(id))
                .clone()
        })
    }

    /// Whether nobody has asked WhatsApp for `id`'s picture yet.
    pub fn avatar_unasked(&self, id: &str) -> bool {
        !self.avatars.contains_key(id) && !self.avatar_requests.contains(id)
    }

    /// Queues an action for the next turn, as a view does.
    pub fn queue(&mut self, action: Action) {
        self.actions.push(action);
    }

    /// Notes typing in the composer: the reader is active, and the chat
    /// shows "typing…" to the others while it lasts.
    pub fn frontend_keystroke(&mut self) {
        self.app_lock.note_input();
        self.note_keystroke();
    }

    /// The window is on screen (made, or shown again from the tray): the
    /// tray and the Dock take their clicks from now on, as `attach` has them.
    pub fn frontend_window_shown(&mut self) {
        self.window_hidden = false;
        self.hide_intent = false;
        self.wants_show = false;
        self.tray_wait = None;
        if let Some(dock) = &mut self.dock {
            dock.attach();
        }
        if let Some(tray) = &mut self.tray {
            tray.attach();
        }
    }

    /// What a close of the window means: `true` hides it (the tray or the
    /// Dock brings it back), `false` quits.
    pub fn frontend_close_hides(&mut self) -> bool {
        if self.quit_requested || !self.keeps_running_on_close() {
            return false;
        }
        self.window_gone();
        true
    }
}
