//! The Settings rows that do more than flip a switch, as the egui page has
//! them: the app lock and its password form, the account's privacy on
//! WhatsApp, notification sounds, the profile, the proxy, the GIPHY key,
//! and the blocked contacts dialog.

use std::cell::Cell;
use std::rc::{Rc, Weak};

use awra::prelude::*;

use super::Driver;
use super::dialogs::Live;
use whatsawra::app::App;
use whatsawra::app_lock::{FormError, FormMode, MIN_PASSWORD_CHARS};
use whatsawra::i18n::{Locale, gettext, pgettext};
use whatsawra::model::{Action, Dialog};
use whatsawra::privacy::PrivacyKind;
use whatsawra::settings::{AutoLock, NotificationSound};

fn act(driver: &Weak<Driver>, action: Action) {
    if let Some(driver) = driver.upgrade() {
        driver.act(action);
    }
}

fn row(title: &str, description: &str, control: &impl awra::IsWidget) -> awra::ActionRow {
    let row = awra::ActionRow::new(title, description);
    control.upcast_ref().set_valign(awra::Align::Center);
    row.set_suffix(Some(control));
    row
}

// --- App lock ---------------------------------------------------------------

pub struct AppLock {
    set: awra::Button,
    change: awra::Button,
    off: awra::Button,
    after_row: awra::ActionRow,
    after: awra::Dropdown,
    form: Option<(FormMode, LockForm)>,
    driver: Weak<Driver>,
}

/// The password form, a dialog of its own while the app's form is open.
struct LockForm {
    dialog: awra::Dialog,
    current: awra::PasswordEntry,
    new: awra::PasswordEntry,
    confirm: awra::PasswordEntry,
    error: awra::Label,
}

impl AppLock {
    pub fn build(driver: &Rc<Driver>, locale: Locale, section: &awra::Section) -> Self {
        let t = |text: &'static str| gettext(locale, text).into_owned();
        let buttons = awra::Box::new(awra::Orientation::Horizontal, 6);
        let button = |label: String, mode: FormMode| {
            let button = awra::Button::with_label(&label);
            let weak = Rc::downgrade(driver);
            button.connect_clicked(move |_| act(&weak, Action::AppLockForm(Some(mode))));
            buttons.append(&button);
            button
        };
        let set = button(t("Set password…"), FormMode::Set);
        let change = button(t("Change password…"), FormMode::Change);
        let off = button(t("Turn off…"), FormMode::TurnOff);
        section.append(&row(
            &t("App lock"),
            &t("Asks for a password at start and after a while unused. It keeps people using this computer out of your chats and encrypts nothing more. A forgotten password means unlinking."),
            &buttons,
        ));
        let labels: Vec<String> = AutoLock::ALL
            .iter()
            .map(|after| after.label(locale).into_owned())
            .collect();
        let after =
            awra::Dropdown::from_strings(&labels.iter().map(String::as_str).collect::<Vec<_>>());
        let weak = Rc::downgrade(driver);
        after.connect_selected_notify(move |dropdown| {
            let Some(driver) = weak.upgrade() else { return };
            let Some(chosen) = AutoLock::ALL.get(dropdown.selected() as usize).copied() else {
                return;
            };
            if driver.read(|app| app.settings.app_lock_after != chosen) {
                driver.act(Action::SetAutoLock(chosen));
            }
        });
        let after_row = row(
            &t("Lock after"),
            &t(
                "Time without using WhatsAwra, also counted while it is in the tray. Ctrl+Shift+L locks it at once.",
            ),
            &after,
        );
        section.append(&after_row);
        Self {
            set,
            change,
            off,
            after_row,
            after,
            form: None,
            driver: Rc::downgrade(driver),
        }
    }

    pub fn sync(&mut self, app: &App, window: Option<&awra::Window>) {
        let enabled = app.settings.app_lock_hash.is_some();
        self.set.set_visible(!enabled);
        self.change.set_visible(enabled);
        self.off.set_visible(enabled);
        self.after_row.set_visible(enabled);
        let after = AutoLock::ALL
            .iter()
            .position(|after| *after == app.settings.app_lock_after)
            .unwrap_or(0) as u32;
        if self.after.selected() != after {
            self.after.set_selected(after);
        }
        // The form opens and closes with the app's.
        let wanted = app.app_lock.form.as_ref().map(|form| form.mode);
        if self.form.as_ref().map(|(mode, _)| *mode) != wanted {
            if let Some((_, old)) = self.form.take() {
                old.dialog.set_rust_data("closed-by-app", true);
                old.dialog.close();
            }
            if let Some(mode) = wanted {
                self.form = Some((mode, self.open_form(app.locale, mode, window)));
            }
        }
        if let (Some((_, form)), Some(state)) = (&self.form, app.app_lock.form.as_ref()) {
            let locale = app.locale;
            let error = state.error.map(|error| match error {
                FormError::TooShort => {
                    gettext(locale, "The password needs at least {count} characters.")
                        .replace("{count}", &MIN_PASSWORD_CHARS.to_string())
                }
                FormError::Mismatch => {
                    gettext(locale, "The two new passwords are different.").into_owned()
                }
                FormError::WrongCurrent => {
                    gettext(locale, "Wrong password. Try again.").into_owned()
                }
            });
            form.error.set_visible(error.is_some());
            form.error.set_text(error.as_deref().unwrap_or_default());
            form.dialog.set_response_enabled("submit", !state.busy);
            // The fields are the view's: they go to the app as they change,
            // and an empty one in the app (after a submit) empties here.
            for (entry, text) in [
                (&form.current, &state.current),
                (&form.new, &state.new),
                (&form.confirm, &state.confirm),
            ] {
                if text.is_empty() && !entry.text().is_empty() {
                    entry.set_text("");
                }
            }
        }
    }

    fn open_form(&self, locale: Locale, mode: FormMode, window: Option<&awra::Window>) -> LockForm {
        let t = |text: &'static str| gettext(locale, text).into_owned();
        let dialog = awra::Dialog::new(window);
        dialog.set_title(Some(
            t(match mode {
                FormMode::Set => "Set an app lock password",
                FormMode::Change => "Change the app lock password",
                FormMode::TurnOff => "Turn off the app lock",
            })
            .as_str(),
        ));
        dialog.set_content_width(400);
        let content = awra::Box::new(awra::Orientation::Vertical, 8);
        content.set_margin_start(16);
        content.set_margin_end(16);
        content.set_margin_bottom(8);
        let field = |placeholder: String, write: fn(&mut whatsawra::app_lock::Form, String)| {
            let entry = awra::PasswordEntry::new();
            entry.set_placeholder_text(placeholder);
            let weak = self.driver.clone();
            entry.delegate().connect_changed(move |entry| {
                let text = entry.text();
                if let Some(driver) = weak.upgrade() {
                    driver.edit(move |app| {
                        if let Some(form) = app.app_lock.form.as_mut() {
                            write(form, text);
                        }
                    });
                }
            });
            let weak = self.driver.clone();
            entry.connect_activate(move |_| act(&weak, Action::SubmitAppLockForm));
            content.append(&entry);
            entry
        };
        let current = field(t("Current password"), |form, text| form.current = text);
        let new = field(
            gettext(locale, "New password, at least {count} characters")
                .replace("{count}", &MIN_PASSWORD_CHARS.to_string()),
            |form, text| form.new = text,
        );
        let confirm = field(t("Type the new password again"), |form, text| {
            form.confirm = text
        });
        current.set_visible(mode != FormMode::Set);
        new.set_visible(mode != FormMode::TurnOff);
        confirm.set_visible(mode != FormMode::TurnOff);
        let error = awra::Label::new(None);
        error.set_xalign(0.0);
        error.set_wrap(true);
        awra::add_style_role(&error, "quiet-destructive");
        content.append(&error);
        dialog.set_child(Some(&content));
        dialog.add_response("cancel", &t("Cancel"));
        dialog.add_response(
            "submit",
            &t(match mode {
                FormMode::Set => "Turn on",
                FormMode::Change => "Change password",
                FormMode::TurnOff => "Turn off",
            }),
        );
        dialog.set_response_appearance(
            "submit",
            if mode == FormMode::TurnOff {
                awra::ButtonAppearance::Destructive
            } else {
                awra::ButtonAppearance::Primary
            },
        );
        dialog.set_response_closes("submit", false);
        dialog.set_close_response(Some("cancel"));
        let weak = self.driver.clone();
        dialog.connect_response(None, move |_, response| {
            act(
                &weak,
                if response == "submit" {
                    Action::SubmitAppLockForm
                } else {
                    Action::AppLockForm(None)
                },
            );
        });
        let weak = self.driver.clone();
        dialog.connect_close_request(move |dialog| {
            if !dialog.rust_data::<bool>("closed-by-app").unwrap_or(false) {
                act(&weak, Action::AppLockForm(None));
            }
            awra::glib::Propagation::Proceed
        });
        dialog.present();
        if mode == FormMode::Set {
            new.grab_focus();
        } else {
            current.grab_focus();
        }
        LockForm {
            dialog,
            current,
            new,
            confirm,
            error,
        }
    }
}

// --- Account privacy --------------------------------------------------------

pub struct AccountPrivacy {
    note: awra::Label,
    rows: Vec<(PrivacyKind, awra::ActionRow, awra::Dropdown)>,
}

impl AccountPrivacy {
    pub fn build(driver: &Rc<Driver>, locale: Locale, section: &awra::Section) -> Self {
        let note = awra::Label::new(None);
        note.set_xalign(0.0);
        note.set_wrap(true);
        note.set_typography(awra::Typography::Muted);
        section.append(&note);
        let mut rows = Vec::new();
        for kind in PrivacyKind::ALL {
            let labels: Vec<String> = kind
                .choices()
                .iter()
                .map(|choice| choice.label(locale).into_owned())
                .collect();
            let dropdown = awra::Dropdown::from_strings(
                &labels.iter().map(String::as_str).collect::<Vec<_>>(),
            );
            let weak = Rc::downgrade(driver);
            dropdown.connect_selected_notify(move |dropdown| {
                let Some(driver) = weak.upgrade() else { return };
                let Some(choice) = kind.choices().get(dropdown.selected() as usize).copied() else {
                    return;
                };
                if driver.read(|app| app.account_privacy.get(kind) != Some(choice)) {
                    driver.act(Action::SetAccountPrivacy { kind, choice });
                }
            });
            let shown = row(&kind.label(locale), &kind.hint(locale), &dropdown);
            section.append(&shown);
            rows.push((kind, shown, dropdown));
        }
        Self { note, rows }
    }

    pub fn sync(&self, app: &App) {
        let locale = app.locale;
        let editable = app.is_connected() && app.account_privacy.editable();
        let note = if app.account_privacy.fetch_failed {
            gettext(
                locale,
                "Could not load your account privacy. Trying again when WhatsAwra reconnects.",
            )
            .into_owned()
        } else if !app.is_connected() {
            gettext(
                locale,
                "Connect to WhatsApp to change your account privacy.",
            )
            .into_owned()
        } else if !app.account_privacy.loaded {
            gettext(locale, "Loading your account privacy…").into_owned()
        } else {
            String::new()
        };
        self.note.set_visible(!note.is_empty());
        self.note.set_text(&note);
        for (kind, shown, dropdown) in &self.rows {
            let current = app.account_privacy.get(*kind);
            shown.set_visible(current.is_some());
            let Some(current) = current else { continue };
            dropdown.set_sensitive(editable && !app.account_privacy.pending(*kind));
            if let Some(index) = kind.choices().iter().position(|choice| *choice == current)
                && dropdown.selected() != index as u32
            {
                dropdown.set_selected(index as u32);
            }
            let mut hint = kind.hint(locale).into_owned();
            if current == whatsawra::privacy::PrivacyChoice::Except {
                let note = gettext(locale, "Change who is excluded on your phone.");
                hint = if hint.is_empty() {
                    note.into_owned()
                } else {
                    format!("{hint} {note}")
                };
            }
            shown.set_subtitle(Some(hint.as_str()));
        }
    }
}

// --- Notification sounds ----------------------------------------------------

/// The fixed sounds, in the menu's order; a file is the last entry.
fn sounds() -> [NotificationSound; 4] {
    [
        NotificationSound::Receive,
        NotificationSound::Alert,
        NotificationSound::System,
        NotificationSound::None,
    ]
}

pub struct Sounds {
    rows: Vec<(bool, awra::ActionRow, awra::Dropdown, awra::Button)>,
}

impl Sounds {
    pub fn build(driver: &Rc<Driver>, locale: Locale, section: &awra::Section) -> Self {
        let t = |text: &'static str| gettext(locale, text).into_owned();
        let labels = [
            "Pidgin".to_owned(),
            t("Pidgin alert"),
            t("System default"),
            pgettext(locale, "sound", "None").into_owned(),
            t("Choose a file…"),
        ];
        let mut rows = Vec::new();
        for (mention, title, description) in [
            (false, t("Message sound"), String::new()),
            (
                true,
                t("Mention sound"),
                t("When a group mentions you or replies to you."),
            ),
        ] {
            let dropdown = awra::Dropdown::from_strings(&labels.each_ref().map(String::as_str));
            let weak = Rc::downgrade(driver);
            dropdown.connect_selected_notify(move |dropdown| {
                let Some(driver) = weak.upgrade() else { return };
                let current = driver.read(|app| {
                    if mention {
                        app.settings.mention_sound.clone()
                    } else {
                        app.settings.message_sound.clone()
                    }
                });
                match sounds().get(dropdown.selected() as usize) {
                    Some(sound) if *sound != current => {
                        driver.act(Action::SetNotificationSound {
                            mention,
                            sound: sound.clone(),
                        });
                    }
                    Some(_) => {}
                    None if !matches!(current, NotificationSound::Custom(_)) => {
                        driver.act(Action::PickNotificationSound { mention });
                    }
                    None => {}
                }
            });
            let play = awra::Button::from_icon_name("media-playback-start-symbolic");
            play.set_appearance(awra::ButtonAppearance::Ghost);
            play.set_tooltip_text(Some(t("Play").as_str()));
            let weak = Rc::downgrade(driver);
            play.connect_clicked(move |_| {
                let Some(driver) = weak.upgrade() else { return };
                let sound = driver.read(|app| {
                    if mention {
                        app.settings.mention_sound.clone()
                    } else {
                        app.settings.message_sound.clone()
                    }
                });
                driver.act(Action::PreviewSound(sound));
            });
            let controls = awra::Box::new(awra::Orientation::Horizontal, 6);
            controls.append(&play);
            controls.append(&dropdown);
            let shown = row(&title, &description, &controls);
            section.append(&shown);
            rows.push((mention, shown, dropdown, play));
        }
        Self { rows }
    }

    pub fn sync(&self, app: &App) {
        for (mention, shown, dropdown, play) in &self.rows {
            let current = if *mention {
                &app.settings.mention_sound
            } else {
                &app.settings.message_sound
            };
            let index = sounds()
                .iter()
                .position(|sound| sound == current)
                .unwrap_or(sounds().len());
            if dropdown.selected() != index as u32 {
                dropdown.set_selected(index as u32);
            }
            play.set_visible(!matches!(
                current,
                NotificationSound::System | NotificationSound::None
            ));
            if let NotificationSound::Custom(path) = current {
                let name = path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default();
                shown.set_subtitle(Some(name.as_str()));
            }
        }
    }
}

// --- One line of text, applied with a button ---------------------------------

/// An entry with an Apply button: the proxy, the GIPHY key.
pub struct TextSetting {
    entry: awra::Entry,
    get: fn(&App) -> String,
    /// What the field showed last, so the reader's typing is left alone.
    shown: String,
}

impl TextSetting {
    /// `labels` are the row's title, its description and the field's
    /// placeholder.
    pub fn build(
        driver: &Rc<Driver>,
        locale: Locale,
        section: &awra::Section,
        [title, description, placeholder]: [&str; 3],
        get: fn(&App) -> String,
        apply: fn(&mut App, String),
    ) -> Self {
        let entry = awra::Entry::new();
        entry.set_placeholder_text(placeholder);
        entry.set_width_chars(24);
        let button = awra::Button::with_label(&gettext(locale, "Apply"));
        let (weak, field) = (Rc::downgrade(driver), entry.clone());
        button.connect_clicked(move |_| {
            let text = field.text().trim().to_owned();
            if let Some(driver) = weak.upgrade() {
                driver.edit(move |app| apply(app, text));
            }
        });
        let controls = awra::Box::new(awra::Orientation::Horizontal, 6);
        controls.append(&entry);
        controls.append(&button);
        section.append(&row(title, description, &controls));
        Self {
            entry,
            get,
            shown: String::new(),
        }
    }

    pub fn sync(&mut self, app: &App) {
        let value = (self.get)(app);
        if value != self.shown {
            self.entry.set_text(&value);
            self.shown = value;
        }
    }
}

// --- Profile -----------------------------------------------------------------

pub struct Profile {
    row: awra::ActionRow,
    face: awra::Avatar,
    shown_face: Option<std::path::PathBuf>,
}

impl Profile {
    pub fn build(driver: &Rc<Driver>, locale: Locale, section: &awra::Section) -> Self {
        let t = |text: &'static str| gettext(locale, text).into_owned();
        let row = awra::ActionRow::new("", "");
        let face = awra::Avatar::new(40, None);
        row.set_prefix(Some(&face));
        let controls = awra::Box::new(awra::Orientation::Horizontal, 6);
        let photo = awra::Button::with_label(&t("Change photo"));
        let weak = Rc::downgrade(driver);
        photo.connect_clicked(move |_| act(&weak, Action::PickProfilePicture));
        let edit = awra::Button::with_label(&t("Edit…"));
        let weak = Rc::downgrade(driver);
        edit.connect_clicked(move |_| {
            if let Some(driver) = weak.upgrade() {
                edit_profile(&driver);
            }
        });
        controls.append(&photo);
        controls.append(&edit);
        controls.set_valign(awra::Align::Center);
        row.set_suffix(Some(&controls));
        section.append(&row);
        Self {
            row,
            face,
            shown_face: None,
        }
    }

    pub fn sync(&mut self, app: &App) {
        let name = app.me_name.clone().unwrap_or_else(|| app.self_title());
        self.row.set_title(Some(name.as_str()));
        let about = app.me_about.clone().unwrap_or_default();
        let phone = app.phone().unwrap_or_default();
        let subtitle = if about.is_empty() {
            phone
        } else {
            format!("{about}\n{phone}")
        };
        self.row.set_subtitle(Some(subtitle.as_str()));
        self.face.set_text(Some(name.as_str()));
        let picture = app.me.as_deref().and_then(|me| app.avatar_path(me));
        if picture != self.shown_face {
            super::pictures::avatar(&self.face, picture.as_deref());
            self.shown_face = picture;
        }
    }
}

/// Our name and About text, in a dialog: Save sends what changed.
fn edit_profile(driver: &Rc<Driver>) {
    let (locale, name, about) = driver.read(|app| {
        (
            app.locale,
            app.me_name.clone().unwrap_or_default(),
            app.me_about.clone().unwrap_or_default(),
        )
    });
    let t = |text: &'static str| gettext(locale, text).into_owned();
    let window = driver
        .shell
        .borrow()
        .as_ref()
        .map(|shell| shell.window.clone());
    let dialog = awra::Dialog::new(window.as_ref());
    dialog.set_title(Some(t("Your profile").as_str()));
    dialog.set_content_width(400);
    let content = awra::Box::new(awra::Orientation::Vertical, 8);
    content.set_margin_start(16);
    content.set_margin_end(16);
    content.set_margin_bottom(8);
    let name_entry = awra::Entry::new();
    name_entry.set_placeholder_text(t("Your name"));
    name_entry.set_text(&name);
    let about_entry = awra::Entry::new();
    about_entry.set_placeholder_text(t("About"));
    about_entry.set_text(&about);
    content.append(&name_entry);
    content.append(&about_entry);
    dialog.set_child(Some(&content));
    dialog.add_response("cancel", &t("Cancel"));
    dialog.add_response("save", &t("Save"));
    dialog.set_response_appearance("save", awra::ButtonAppearance::Primary);
    dialog.set_close_response(Some("cancel"));
    let weak = Rc::downgrade(driver);
    dialog.connect_response(Some("save"), move |_, _| {
        let (new_name, new_about) = (name_entry.text(), about_entry.text());
        act(
            &weak,
            Action::SetProfile {
                name: (new_name.trim() != name.trim()).then(|| new_name.trim().to_owned()),
                about: (new_about.trim() != about.trim()).then(|| new_about.trim().to_owned()),
            },
        );
    });
    dialog.present();
}

// --- Blocked contacts ---------------------------------------------------------

pub fn dialog_of(
    driver: &Rc<Driver>,
    app: &App,
    window: &awra::Window,
    dialog: &Dialog,
) -> Option<Box<dyn Live>> {
    match dialog {
        Dialog::BlockedContacts => Some(Box::new(Blocked::new(driver, app, window))),
        _ => None,
    }
}

struct Blocked {
    dialog: awra::Dialog,
    status: awra::Label,
    retry: awra::Button,
    list: awra::Box,
    shown: Option<Vec<(String, String, bool)>>,
    driver: Weak<Driver>,
}

impl Blocked {
    fn new(driver: &Rc<Driver>, app: &App, window: &awra::Window) -> Self {
        let t = |text: &'static str| gettext(app.locale, text).into_owned();
        let dialog = awra::Dialog::new(Some(window));
        dialog.set_title(Some(t("Blocked contacts").as_str()));
        dialog.set_content_width(420);
        dialog.set_default_size(440, 520);
        let content = awra::Box::new(awra::Orientation::Vertical, 8);
        content.set_margin_start(16);
        content.set_margin_end(16);
        content.set_margin_bottom(8);
        let status = awra::Label::new(None);
        status.set_xalign(0.0);
        status.set_wrap(true);
        status.set_typography(awra::Typography::Muted);
        content.append(&status);
        let retry = awra::Button::with_label(&t("Try again"));
        retry.set_halign(awra::Align::Start);
        let weak = Rc::downgrade(driver);
        retry.connect_clicked(move |_| act(&weak, Action::FetchBlocklist));
        content.append(&retry);
        let list = awra::Box::new(awra::Orientation::Vertical, 2);
        let scrolled = awra::ScrolledWindow::new();
        scrolled.set_hscrollbar_policy(awra::PolicyType::Never);
        scrolled.set_child(Some(&list));
        scrolled.set_vexpand(true);
        scrolled.set_min_content_height(280);
        content.append(&scrolled);
        dialog.set_child(Some(&content));
        dialog.add_response("close", &t("Close"));
        dialog.set_close_response(Some("close"));
        let answered = Rc::new(Cell::new(false));
        let (weak, replied) = (Rc::downgrade(driver), answered.clone());
        dialog.connect_response(None, move |_, _| {
            replied.set(true);
            act(&weak, Action::CloseDialog);
        });
        super::dialogs::close_on_dismiss(driver, &dialog, answered);
        Self {
            dialog,
            status,
            retry,
            list,
            shown: None,
            driver: Rc::downgrade(driver),
        }
    }
}

impl Live for Blocked {
    fn dialog(&self) -> awra::Dialog {
        self.dialog.clone()
    }

    fn sync(&mut self, _driver: &Rc<Driver>, app: &App) {
        let locale = app.locale;
        let Some(ids) = app.blocklist.ids.as_ref() else {
            self.status.set_visible(true);
            self.status
                .set_text(&gettext(locale, "Could not load your blocked contacts."));
            self.retry.set_visible(true);
            self.retry.set_sensitive(app.is_connected());
            return;
        };
        self.retry.set_visible(false);
        let mut rows: Vec<(String, String, bool)> = ids
            .iter()
            .map(|id| {
                (
                    id.clone(),
                    whatsawra::ui::blocking::name(app, id),
                    whatsawra::ui::blocking::hidden(app, id),
                )
            })
            .collect();
        rows.sort_by_key(|row| row.1.to_lowercase());
        self.status.set_visible(rows.is_empty());
        self.status
            .set_text(&gettext(locale, "No blocked contacts"));
        if self.shown.as_ref() == Some(&rows) {
            return;
        }
        while let Some(child) = self.list.first_child() {
            self.list.remove(&child);
        }
        let editable = app.is_connected() && app.blocklist.editable();
        for (id, name, hidden) in &rows {
            let line = awra::Box::new(awra::Orientation::Horizontal, 10);
            let face = awra::Avatar::new(32, Some(name));
            if !hidden {
                super::pictures::avatar(&face, app.avatar_path(id).as_deref());
            }
            let label = awra::Label::new(Some(name));
            label.set_xalign(0.0);
            label.set_hexpand(true);
            label.set_ellipsize(awra::pango::EllipsizeMode::End);
            let unblock = awra::Button::with_label(&gettext(locale, "Unblock"));
            unblock.set_sensitive(editable);
            let (weak, chat) = (self.driver.clone(), id.clone());
            unblock.connect_clicked(move |_| {
                act(
                    &weak,
                    Action::ShowDialog(Dialog::ConfirmBlock {
                        chat: chat.clone(),
                        blocked: false,
                        from_list: true,
                    }),
                );
            });
            line.append(&face);
            line.append(&label);
            line.append(&unblock);
            self.list.append(&line);
        }
        self.shown = Some(rows);
    }
}
