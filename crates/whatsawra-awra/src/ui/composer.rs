//! What sits around the composer's text: the attachments staged to go with
//! it, and a voice message while the microphone records.

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
