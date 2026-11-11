//! The app lock's screen: while WhatsAwra is locked the window shows this
//! and nothing else, as the egui window does. The password goes into the
//! app's own entry field; the app checks it and drops every other action.

use std::rc::Rc;

use awra::prelude::*;

use super::Driver;
use whatsawra::app::App;
use whatsawra::app_lock::Forgetting;
use whatsawra::i18n::{Locale, gettext};
use whatsawra::model::Action;

pub struct LockScreen {
    pub root: awra::Box,
    password: awra::PasswordEntry,
    unlock: awra::Button,
    status: awra::Label,
    ask: awra::Box,
    confirm: awra::Box,
    unlinking: awra::Box,
}

impl LockScreen {
    pub fn build(driver: &Rc<Driver>, locale: Locale) -> Self {
        let t = |text: &'static str| gettext(locale, text).into_owned();
        let root = awra::Box::new(awra::Orientation::Vertical, 14);
        root.set_halign(awra::Align::Center);
        root.set_valign(awra::Align::Center);
        root.set_size_request(360, -1);

        let icon = awra::Image::from_icon_name("changes-prevent-symbolic");
        icon.set_pixel_size(48);
        root.append(&icon);
        let title = awra::Label::new(Some(&t("WhatsAwra is locked")));
        title.set_typography(awra::Typography::Title2);
        root.append(&title);

        // Asking for the password.
        let ask = awra::Box::new(awra::Orientation::Vertical, 10);
        let hint = awra::Label::new(Some(&t("Enter your password to unlock.")));
        hint.set_typography(awra::Typography::Muted);
        ask.append(&hint);
        let password = awra::PasswordEntry::new();
        password.set_placeholder_text(Some(t("Password").as_str()));
        ask.append(&password);
        let status = awra::Label::new(None);
        status.set_wrap(true);
        status.set_visible(false);
        ask.append(&status);
        let unlock = awra::Button::with_label(&t("Unlock"));
        unlock.set_appearance(awra::ButtonAppearance::Primary);
        ask.append(&unlock);
        let forgot = awra::Button::with_label(&t("Forgot password? Unlink this computer"));
        forgot.set_appearance(awra::ButtonAppearance::Ghost);
        ask.append(&forgot);
        root.append(&ask);

        let weak = Rc::downgrade(driver);
        password.connect_changed(move |entry| {
            let text = entry.text();
            if let Some(driver) = weak.upgrade() {
                driver.edit(move |app| app.app_lock.entry = text);
            }
        });
        let submit = {
            let weak = Rc::downgrade(driver);
            move || {
                if let Some(driver) = weak.upgrade() {
                    driver.act(Action::UnlockApp);
                }
            }
        };
        let on_enter = submit.clone();
        password.connect_activate(move |_| on_enter());
        unlock.connect_clicked(move |_| submit());
        let weak = Rc::downgrade(driver);
        forgot.connect_clicked(move |_| {
            if let Some(driver) = weak.upgrade() {
                driver.act(Action::ForgotAppPassword(true));
            }
        });

        // Unlinking is the only way back without the password.
        let confirm = awra::Box::new(awra::Orientation::Vertical, 10);
        let heading = awra::Label::new(Some(&t("Unlink this computer?")));
        heading.set_weight(600);
        let body = awra::Label::new(Some(&t(
            "Without the password, the only way back in is to unlink. This removes the device from WhatsApp, deletes the chats stored here, and turns the app lock off. You can link again with a new code.",
        )));
        body.set_wrap(true);
        body.set_max_width_chars(44);
        let buttons = awra::Box::new(awra::Orientation::Horizontal, 8);
        buttons.set_halign(awra::Align::End);
        let cancel = awra::Button::with_label(&t("Cancel"));
        let unlink = awra::Button::with_label(&t("Unlink"));
        unlink.set_appearance(awra::ButtonAppearance::Destructive);
        buttons.append(&cancel);
        buttons.append(&unlink);
        confirm.append(&heading);
        confirm.append(&body);
        confirm.append(&buttons);
        root.append(&confirm);
        let weak = Rc::downgrade(driver);
        cancel.connect_clicked(move |_| {
            if let Some(driver) = weak.upgrade() {
                driver.act(Action::ForgotAppPassword(false));
            }
        });
        let weak = Rc::downgrade(driver);
        unlink.connect_clicked(move |_| {
            if let Some(driver) = weak.upgrade() {
                driver.act(Action::UnlinkLockedApp);
            }
        });

        let unlinking = awra::Box::new(awra::Orientation::Vertical, 8);
        unlinking.append(&awra::Spinner::new());
        unlinking.append(&awra::Label::new(Some(&t("Unlinking this computer…"))));
        root.append(&unlinking);

        Self {
            root,
            password,
            unlock,
            status,
            ask,
            confirm,
            unlinking,
        }
    }

    pub fn sync(&self, app: &App) {
        let lock = &app.app_lock;
        self.ask.set_visible(lock.forgetting == Forgetting::No);
        self.confirm
            .set_visible(lock.forgetting == Forgetting::Confirming);
        self.unlinking
            .set_visible(lock.forgetting == Forgetting::Unlinking);
        if self.password.text() != lock.entry {
            self.password.set_text(&lock.entry);
        }
        if lock.wait_left().is_some() {
            // The countdown moves on its own.
            super::wake_after(std::time::Duration::from_millis(250));
        }
        let status = if let Some(wait) = lock.wait_left() {
            Some(
                gettext(
                    app.locale,
                    "Too many wrong passwords. Try again in {seconds} s.",
                )
                .replace(
                    "{seconds}",
                    &(wait.as_secs_f32().ceil().max(1.0) as u64).to_string(),
                ),
            )
        } else if lock.wrong {
            Some(gettext(app.locale, "Wrong password. Try again.").into_owned())
        } else {
            None
        };
        self.status.set_visible(status.is_some());
        if let Some(text) = status {
            self.status.set_markup(&format!(
                "<span foreground=\"#e5484d\">{}</span>",
                awra::glib::markup_escape_text(&text)
            ));
        }
        self.unlock.set_sensitive(lock.can_try());
        self.unlock.set_busy(lock.checking());
    }
}
