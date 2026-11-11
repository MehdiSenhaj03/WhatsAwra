//! What sits around the composer's text: the attachments staged to go with
//! it, a voice message while the microphone records, and the selection's
//! actions while messages are being selected.

use std::rc::Rc;

use awra::prelude::*;

use super::Driver;
use whatsawra::app::{App, Pending};
use whatsawra::i18n::{Locale, gettext};
use whatsawra::model::Action;

/// The strip of staged attachments, each with a button that takes it off.
pub struct Attachments {
    pub root: awra::Box,
    shown: Vec<String>,
}

impl Attachments {
    pub fn new() -> Self {
        let root = awra::Box::new(awra::Orientation::Horizontal, 6);
        root.set_margin_top(4);
        root.set_margin_start(8);
        root.set_margin_end(8);
        Self {
            root,
            shown: Vec::new(),
        }
    }

    /// Shows `app.pending`; returns whether anything is staged.
    pub fn sync(&mut self, driver: &Rc<Driver>, app: &App) -> bool {
        let names: Vec<String> = app
            .pending
            .iter()
            .map(|staged| match &staged.item {
                Pending::Picture { .. } => gettext(app.locale, "Pasted picture").into_owned(),
                Pending::File(path) | Pending::Document(path) => path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            })
            .collect();
        if names != self.shown {
            while let Some(child) = self.root.first_child() {
                self.root.remove(&child);
            }
            for (index, name) in names.iter().enumerate() {
                let chip = awra::Chip::new(Some(name));
                chip.set_icon_name(Some("x-office-document-symbolic"));
                chip.set_toggleable(false);
                chip.set_tooltip_text(Some(gettext(app.locale, "Remove").as_ref()));
                let weak = Rc::downgrade(driver);
                chip.connect_activate(move |_| {
                    if let Some(driver) = weak.upgrade() {
                        driver.act(Action::RemovePending(index));
                    }
                });
                self.root.append(&chip);
            }
            self.shown = names;
        }
        // An empty strip would still take its room above the text.
        self.root.set_visible(!self.shown.is_empty());
        !self.shown.is_empty()
    }
}

/// The microphone button and, while recording, its time, bars and
/// the ways out (cancel, send).
pub struct Recording {
    pub mic: awra::Button,
    pub bar: awra::Box,
    time: awra::Label,
    waveform: awra::WaveformView,
}

impl Recording {
    pub fn build(driver: &Rc<Driver>, locale: Locale) -> Self {
        let t = |text: &'static str| gettext(locale, text).into_owned();
        let mic = awra::Button::from_icon_name("awra-microphone-symbolic");
        mic.set_appearance(awra::ButtonAppearance::Toolbar);
        mic.set_tooltip_text(Some(t("Record a voice message").as_str()));
        let weak = Rc::downgrade(driver);
        mic.connect_clicked(move |_| {
            if let Some(driver) = weak.upgrade() {
                driver.act(Action::StartRecording);
            }
        });

        let bar = awra::Box::new(awra::Orientation::Horizontal, 10);
        bar.set_margin_start(12);
        bar.set_margin_end(12);
        bar.set_margin_bottom(12);
        let cancel = awra::Button::from_icon_name("user-trash-symbolic");
        cancel.set_appearance(awra::ButtonAppearance::Ghost);
        cancel.set_tooltip_text(Some(t("Cancel").as_str()));
        cancel.set_valign(awra::Align::Center);
        let dot = awra::Image::from_icon_name("media-record-symbolic");
        dot.set_valign(awra::Align::Center);
        let time = awra::Label::new(None);
        time.set_tabular_digits(true);
        let waveform = awra::WaveformView::new();
        waveform.set_hexpand(true);
        waveform.set_size_request(-1, 36);
        let send = awra::Button::from_icon_name("awra-send-symbolic");
        send.set_appearance(awra::ButtonAppearance::Primary);
        send.set_tooltip_text(Some(t("Send").as_str()));
        send.set_valign(awra::Align::Center);
        bar.append(&cancel);
        bar.append(&dot);
        bar.append(&time);
        bar.append(&waveform);
        bar.append(&send);
        let weak = Rc::downgrade(driver);
        cancel.connect_clicked(move |_| {
            if let Some(driver) = weak.upgrade() {
                driver.act(Action::CancelRecording);
            }
        });
        let weak = Rc::downgrade(driver);
        send.connect_clicked(move |_| {
            if let Some(driver) = weak.upgrade() {
                driver.act(Action::SendRecording);
            }
        });
        Self {
            mic,
            bar,
            time,
            waveform,
        }
    }

    /// Shows the recording if there is one; returns whether there is.
    pub fn sync(&self, app: &App, composer_empty: bool) -> bool {
        let Some(recorder) = app.recording.as_ref() else {
            self.bar.set_visible(false);
            self.mic
                .set_visible(composer_empty && app.editing.is_none());
            return false;
        };
        self.mic.set_visible(false);
        self.bar.set_visible(true);
        let elapsed = recorder.elapsed();
        self.time
            .set_text(&whatsawra::util::duration(elapsed.as_secs() as u32));
        let levels = recorder.levels();
        let shown = &levels[levels.len().saturating_sub(64)..];
        self.waveform.set_peaks(shown);
        // The time and bars move while the microphone listens.
        awra::glib::timeout_add_local_once(std::time::Duration::from_millis(100), super::wake);
        true
    }
}

/// In the composer's place while messages are selected: how many, and what
/// to do with them.
pub struct SelectionBar {
    pub root: awra::Box,
    count: awra::Label,
    forward: awra::Button,
    delete: awra::Button,
}

impl SelectionBar {
    pub fn build(driver: &Rc<Driver>, locale: Locale) -> Self {
        let t = |text: &'static str| gettext(locale, text).into_owned();
        let root = awra::Box::new(awra::Orientation::Horizontal, 8);
        root.set_margin_start(12);
        root.set_margin_end(12);
        root.set_margin_bottom(12);
        root.set_margin_top(4);
        let cancel = awra::Button::from_icon_name("window-close-symbolic");
        cancel.set_appearance(awra::ButtonAppearance::Toolbar);
        cancel.set_tooltip_text(Some(t("Cancel selection").as_str()));
        let weak = Rc::downgrade(driver);
        cancel.connect_clicked(move |_| {
            if let Some(driver) = weak.upgrade() {
                driver.act(Action::CancelSelection);
            }
        });
        let count = awra::Label::new(None);
        count.set_xalign(0.0);
        count.set_hexpand(true);
        count.set_weight(600);
        let delete = awra::Button::with_label(&t("Delete…"));
        delete.set_appearance(awra::ButtonAppearance::Secondary);
        let weak = Rc::downgrade(driver);
        delete.connect_clicked(move |button| {
            let Some(driver) = weak.upgrade() else { return };
            let entries = driver.read(|app| {
                let Some((chat, selected)) = app.selection.clone() else {
                    return Vec::new();
                };
                let can_revoke = app.conversations.get(&chat).is_some_and(|conversation| {
                    whatsawra::ui::conversation::selection_can_revoke(
                        &conversation.messages,
                        &selected,
                        whatsawra::util::now(),
                    )
                });
                [
                    (gettext(app.locale, "Delete for everyone"), true, can_revoke),
                    (
                        gettext(app.locale, "Delete for me"),
                        false,
                        !selected.is_empty(),
                    ),
                ]
                .into_iter()
                .map(|(label, for_everyone, enabled)| {
                    let action =
                        Action::ShowDialog(whatsawra::model::Dialog::ConfirmDeleteSelection {
                            chat: chat.clone(),
                            messages: selected.clone(),
                            for_everyone,
                        });
                    let weak = Rc::downgrade(&driver);
                    awra::menu::Entry::item(label.into_owned(), move || {
                        if let Some(driver) = weak.upgrade() {
                            driver.act(action.clone());
                        }
                    })
                    .icon("user-trash-symbolic")
                    .destructive()
                    .sensitive(enabled)
                })
                .collect()
            });
            if !entries.is_empty() {
                awra::menu::popup_at(button, 0.0, 0.0, entries);
            }
        });
        let forward = awra::Button::with_label(&t("Forward…"));
        forward.set_appearance(awra::ButtonAppearance::Primary);
        let weak = Rc::downgrade(driver);
        forward.connect_clicked(move |_| {
            let Some(driver) = weak.upgrade() else { return };
            if let Some((chat, messages)) = driver.read(|app| app.selection.clone()) {
                driver.act(Action::ShowDialog(whatsawra::model::Dialog::Forward {
                    chat,
                    messages,
                }));
            }
        });
        root.append(&cancel);
        root.append(&count);
        root.append(&delete);
        root.append(&forward);
        root.set_visible(false);
        Self {
            root,
            count,
            forward,
            delete,
        }
    }

    /// Shows the bar while `chat`'s messages are being selected; returns
    /// whether it shows.
    pub fn sync(&self, app: &App, chat: &str) -> bool {
        let Some((_, selected)) = app.selection.as_ref().filter(|(open, _)| open == chat) else {
            self.root.set_visible(false);
            return false;
        };
        self.root.set_visible(true);
        self.count.set_text(
            &whatsawra::i18n::ngettext(
                app.locale,
                "{count} selected",
                "{count} selected",
                selected.len() as u32,
            )
            .replace("{count}", &selected.len().to_string()),
        );
        // The selection stays open with nothing in it; there is nothing to
        // act on until one is ticked.
        self.forward.set_sensitive(!selected.is_empty());
        self.delete.set_sensitive(!selected.is_empty());
        true
    }
}
