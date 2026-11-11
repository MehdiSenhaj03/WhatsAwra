//! Linking a phone: the QR code to scan, or a code to type on the phone.

use std::rc::Rc;

use awra::prelude::*;

use super::Driver;
use crate::app::App;
use crate::backend::LinkStatus;
use crate::i18n::{Locale, gettext};
use crate::model::Action;

pub struct Login {
    pub root: awra::Box,
    status: awra::Label,
    qr: awra::Image,
    shown_qr: Option<String>,
    spinner: awra::Spinner,
    pair_code: awra::Label,
    phone: awra::Entry,
}

impl Login {
    pub fn build(driver: &Rc<Driver>, locale: Locale) -> Self {
        let root = awra::Box::new(awra::Orientation::Vertical, 16);
        root.set_halign(awra::Align::Center);
        root.set_valign(awra::Align::Center);

        let card = awra::Card::new();
        card.set_appearance(awra::CardAppearance::Raised);
        let content = awra::Box::new(awra::Orientation::Vertical, 14);
        content.set_margin_top(12);
        content.set_margin_bottom(12);
        content.set_margin_start(12);
        content.set_margin_end(12);

        let title = awra::Label::new(Some(
            gettext(locale, "Link WhatsAwra to your phone").as_ref(),
        ));
        title.set_typography(awra::Typography::Title2);
        content.append(&title);
        let steps = awra::Label::new(Some(
            gettext(
                locale,
                "On your phone, open WhatsApp, then Settings › Linked devices › Link a device, and scan this code.",
            )
            .as_ref(),
        ));
        steps.set_wrap(true);
        steps.set_max_width_chars(44);
        steps.set_justify(awra::Justification::Center);
        steps.set_typography(awra::Typography::Muted);
        content.append(&steps);

        let qr = awra::Image::new();
        qr.set_pixel_size(264);
        qr.set_halign(awra::Align::Center);
        content.append(&qr);
        let spinner = awra::Spinner::new();
        spinner.set_halign(awra::Align::Center);
        content.append(&spinner);

        let status = awra::Label::new(None);
        status.set_typography(awra::Typography::Muted);
        status.set_wrap(true);
        content.append(&status);

        let pair_code = awra::Label::new(None);
        pair_code.set_typography(awra::Typography::Title1);
        pair_code.set_tabular_digits(true);
        content.append(&pair_code);

        let pair = awra::Box::new(awra::Orientation::Horizontal, 8);
        let phone = awra::Entry::new();
        phone.set_placeholder_text(Some(
            gettext(locale, "Phone number with country code").as_ref(),
        ));
        phone.set_hexpand(true);
        let link = awra::Button::with_label(gettext(locale, "Link with phone number").as_ref());
        let weak = Rc::downgrade(driver);
        let entry = phone.clone();
        let pair_now = move || {
            if let Some(driver) = weak.upgrade() {
                driver.act(Action::PairWithPhone(entry.text()));
            }
        };
        let pair_click = pair_now.clone();
        link.connect_clicked(move |_| pair_click());
        phone.connect_activate(move |_| pair_now());
        pair.append(&phone);
        pair.append(&link);
        content.append(&pair);

        card.set_child(Some(&content));
        root.append(&card);
        Self {
            root,
            status,
            qr,
            shown_qr: None,
            spinner,
            pair_code,
            phone,
        }
    }

    pub fn sync(&mut self, app: &App) {
        let locale = app.locale;
        let (qr, code, status) = match &app.link {
            LinkStatus::Unlinked {
                qr,
                pair_code,
                pairing_phone,
            } => (
                qr.clone(),
                pair_code.clone(),
                if pair_code.is_some() {
                    gettext(locale, "Enter this code on your phone").into_owned()
                } else if pairing_phone.is_some() {
                    gettext(locale, "Asking WhatsApp for a code…").into_owned()
                } else {
                    String::new()
                },
            ),
            LinkStatus::Starting | LinkStatus::Connecting => {
                (None, None, gettext(locale, "Connecting…").into_owned())
            }
            LinkStatus::LoggedOut => (
                None,
                None,
                gettext(locale, "This computer was unlinked from your phone.").into_owned(),
            ),
            LinkStatus::Failed(reason) => (None, None, reason.clone()),
            LinkStatus::Disconnected { reason } => (None, None, reason.clone()),
            LinkStatus::Connected => (None, None, String::new()),
        };
        if qr != self.shown_qr {
            match qr.as_deref().and_then(qr_rgba) {
                Some((rgba, size)) => {
                    self.qr.set_from_rgba(&rgba, size, size, size * 4);
                    self.qr.set_visible(true);
                }
                None => self.qr.set_visible(false),
            }
            self.shown_qr = qr.clone();
        }
        self.spinner.set_visible(qr.is_none() && code.is_none());
        self.status.set_text(&status);
        self.status.set_visible(!status.is_empty());
        self.pair_code.set_visible(code.is_some());
        if let Some(code) = code {
            self.pair_code.set_text(&code);
        }
        if self.phone.text() != app.pair_phone && !app.pair_phone.is_empty() {
            self.phone.set_text(&app.pair_phone);
        }
    }
}

/// The QR payload as black modules on white, with a quiet zone, scaled so
/// each module is a few pixels.
fn qr_rgba(payload: &str) -> Option<(Vec<u8>, i32)> {
    let code = qrcode::QrCode::new(payload.as_bytes()).ok()?;
    let width = code.width();
    let colors = code.to_colors();
    let quiet = 4;
    let modules = width + quiet * 2;
    let scale = (264 / modules).max(2);
    let size = modules * scale;
    let mut rgba = vec![255u8; size * size * 4];
    for y in 0..width {
        for x in 0..width {
            if colors[y * width + x] == qrcode::Color::Dark {
                for dy in 0..scale {
                    for dx in 0..scale {
                        let px = (x + quiet) * scale + dx;
                        let py = (y + quiet) * scale + dy;
                        let at = (py * size + px) * 4;
                        rgba[at..at + 3].copy_from_slice(&[0, 0, 0]);
                    }
                }
            }
        }
    }
    Some((rgba, size as i32))
}
