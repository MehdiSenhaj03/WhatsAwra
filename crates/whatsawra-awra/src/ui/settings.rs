//! Settings: Awra's page of sections, each row a switch, a choice or a
//! button, writing the same settings and queueing the same actions as the
//! egui page. The theme is Awra's appearance (system, light, dark): the
//! window keeps Awra's look, so WhatsAwra's own palettes are not offered.

use std::rc::Rc;

use awra::prelude::*;

use super::Driver;
use whatsawra::app::App;
use whatsawra::i18n::{Locale, gettext};
use whatsawra::model::{Action, Dialog};
use whatsawra::settings::ThemeChoice;

/// A switch and how to read and write its setting.
struct Toggle {
    switch: awra::Switch,
    get: fn(&App) -> bool,
}

pub struct SettingsPage {
    pub root: awra::Page,
    toggles: Vec<Toggle>,
    theme: awra::Dropdown,
    language: awra::Dropdown,
    account: awra::ActionRow,
    folder: awra::ActionRow,
    start_at_login: awra::ActionRow,
}

const THEMES: [ThemeChoice; 3] = [ThemeChoice::System, ThemeChoice::Light, ThemeChoice::Dark];

impl SettingsPage {
    pub fn build(driver: &Rc<Driver>, app: &App) -> Self {
        let locale = app.locale;
        let t = |text: &'static str| gettext(locale, text).into_owned();
        let column = awra::Box::new(awra::Orientation::Vertical, 24);
        let mut toggles = Vec::new();

        // Appearance
        let appearance = section(&column, &t("Appearance"));
        let theme_names = [t("System"), t("Light"), t("Dark")];
        let theme = awra::Dropdown::from_strings(&theme_names.each_ref().map(String::as_str));
        let weak = Rc::downgrade(driver);
        theme.connect_selected_notify(move |dropdown| {
            if let (Some(driver), Some(choice)) =
                (weak.upgrade(), THEMES.get(dropdown.selected() as usize))
            {
                driver.act(Action::SetTheme(*choice));
            }
        });
        appearance.append(&row(&t("Theme"), "", &theme));
        let mut languages = vec![t("Automatic")];
        languages.extend(Locale::ALL.iter().map(|locale| locale.label().to_owned()));
        let language =
            awra::Dropdown::from_strings(&languages.iter().map(String::as_str).collect::<Vec<_>>());
        let weak = Rc::downgrade(driver);
        language.connect_selected_notify(move |dropdown| {
            let Some(driver) = weak.upgrade() else { return };
            let chosen = match dropdown.selected() as usize {
                0 => None,
                index => Locale::ALL.get(index - 1).copied(),
            };
            driver.act(Action::SetInterfaceLanguage(chosen));
        });
        appearance.append(&row(&t("Language"), "", &language));

        // Chats
        let chats = section(&column, &t("Chats"));
        toggles.push(toggle(
            driver,
            &chats,
            &t("Enter sends"),
            &t("When off, Ctrl+Enter sends."),
            |app| app.settings.enter_sends,
            |app, on| {
                app.settings.enter_sends = on;
                app.queue(Action::SettingsChanged);
            },
        ));
        toggles.push(toggle(
            driver,
            &chats,
            &t("Download files automatically"),
            &t("Files up to 64 MiB download as they come into view."),
            |app| app.account().settings.auto_download,
            |app, on| {
                app.account_mut().settings.auto_download = on;
                app.account_mut().mark_settings_dirty();
            },
        ));
        toggles.push(toggle(
            driver,
            &chats,
            &t("Fetch older history"),
            &t("Asks your phone in the background for older messages in the open, pinned and ten most recent chats."),
            |app| app.account().settings.history_prefetch,
            |app, on| {
                app.account_mut().settings.history_prefetch = on;
                app.account_mut().mark_settings_dirty();
            },
        ));
        if whatsawra::media_pause::SUPPORTED {
            toggles.push(toggle(
                driver,
                &chats,
                &t("Pause other media while recording or playing"),
                &t("Music and videos in other apps resume afterwards."),
                |app| app.settings.pause_other_media,
                |app, on| {
                    app.settings.pause_other_media = on;
                    app.queue(Action::SettingsChanged);
                },
            ));
        }
        toggles.push(toggle(
            driver,
            &chats,
            &t("Keep chats archived"),
            &t("When off, a new message brings an archived chat back to the list."),
            |app| app.settings.keep_chats_archived,
            |app, on| app.queue(Action::SetKeepChatsArchived(on)),
        ));

        // Notifications
        let notifications = section(&column, &t("Notifications"));
        toggles.push(toggle(
            driver,
            &notifications,
            &t("Desktop notifications"),
            &t("For chats you are not looking at. Muted chats stay quiet."),
            |app| app.account().settings.notifications,
            |app, on| {
                app.account_mut().settings.notifications = on;
                app.account_mut().mark_settings_dirty();
            },
        ));
        toggles.push(toggle(
            driver,
            &notifications,
            &t("Show message content in notifications"),
            &t("When off, they say only “New message”."),
            |app| app.settings.notification_content,
            |app, on| {
                app.settings.notification_content = on;
                app.queue(Action::SettingsChanged);
            },
        ));
        toggles.push(toggle(
            driver,
            &notifications,
            &t("Play sounds for group messages"),
            "",
            |app| app.settings.group_sounds,
            |app, on| {
                app.settings.group_sounds = on;
                app.queue(Action::SettingsChanged);
            },
        ));

        // Privacy
        let privacy = section(&column, &t("Privacy"));
        toggles.push(toggle(
            driver,
            &privacy,
            &t("Send read receipts"),
            &t("Let people see when you read their messages."),
            |app| app.account().settings.send_read_receipts,
            |app, on| {
                app.account_mut().settings.send_read_receipts = on;
                app.account_mut().mark_settings_dirty();
            },
        ));
        toggles.push(toggle(
            driver,
            &privacy,
            &t("Show when you are typing"),
            "",
            |app| app.account().settings.send_typing,
            |app, on| {
                app.account_mut().settings.send_typing = on;
                app.account_mut().mark_settings_dirty();
            },
        ));
        privacy.append(&button_row(
            driver,
            &t("Blocked contacts"),
            "",
            &t("Show"),
            Action::ShowDialog(Dialog::BlockedContacts),
        ));

        // System
        let system = section(&column, &t("System"));
        let start_at_login = awra::ActionRow::new(&t("Start at login"), "");
        let login_switch = awra::Switch::new();
        login_switch.set_valign(awra::Align::Center);
        let weak = Rc::downgrade(driver);
        login_switch.connect_active_notify(move |switch| {
            if let Some(driver) = weak.upgrade() {
                driver.act(Action::SetStartWithSystem(switch.is_active()));
            }
        });
        start_at_login.set_suffix(Some(&login_switch));
        system.append(&start_at_login);
        toggles.push(Toggle {
            switch: login_switch,
            get: |app| app.start_with_system.unwrap_or(false),
        });

        // Files
        let files = section(&column, &t("Files"));
        let folder = awra::ActionRow::new(&t("Download folder"), "");
        let change = awra::Button::with_label(&t("Change…"));
        change.set_valign(awra::Align::Center);
        let weak = Rc::downgrade(driver);
        change.connect_clicked(move |_| {
            if let Some(driver) = weak.upgrade() {
                driver.act(Action::PickDownloadFolder);
            }
        });
        folder.set_suffix(Some(&change));
        files.append(&folder);
        let account_id = app.account().id.clone();
        files.append(&button_row(
            driver,
            &t("Media cache"),
            &t("Photos, videos and files downloaded to this computer."),
            &t("Clear"),
            Action::ShowDialog(Dialog::ConfirmClearMediaCache(account_id)),
        ));

        // Account
        let account_section = section(&column, &t("Account"));
        let account = awra::ActionRow::new("", "");
        account_section.append(&account);
        let unlink = button_row(
            driver,
            &t("Unlink this computer"),
            &t("Removes this computer from your phone's linked devices."),
            &t("Unlink"),
            Action::ShowDialog(Dialog::ConfirmUnlink),
        );
        account_section.append(&unlink);

        // About
        let about = section(&column, &t("About"));
        let version = awra::ActionRow::new(
            "WhatsAwra",
            &format!("{} {} · Awra", t("Version"), env!("CARGO_PKG_VERSION")),
        );
        about.append(&version);
        about.append(&button_row(
            driver,
            &t("Log file"),
            &t("Attach it to a bug report. It never contains your messages."),
            &t("Open"),
            Action::OpenLog(app.dirs.log_file()),
        ));

        let root = awra::Page::new();
        root.set_max_content_width(720);
        root.set_child(Some(&column));
        Self {
            root,
            toggles,
            theme,
            language,
            account,
            folder,
            start_at_login,
        }
    }

    pub fn sync(&self, app: &App) {
        for toggle in &self.toggles {
            let on = (toggle.get)(app);
            if toggle.switch.is_active() != on {
                toggle.switch.set_active(on);
            }
        }
        let theme = THEMES
            .iter()
            .position(|choice| *choice == app.settings.theme)
            .unwrap_or(0) as u32;
        if self.theme.selected() != theme {
            self.theme.set_selected(theme);
        }
        let language = app
            .settings
            .interface_language
            .and_then(|chosen| Locale::ALL.iter().position(|locale| *locale == chosen))
            .map_or(0, |index| index as u32 + 1);
        if self.language.selected() != language {
            self.language.set_selected(language);
        }
        let name = app.me_name.clone().unwrap_or_else(|| app.self_title());
        self.account.set_title(Some(name.as_str()));
        self.account
            .set_subtitle(Some(app.phone().unwrap_or_default().as_str()));
        let folder = app.settings.download_folder.as_ref().map_or_else(
            || gettext(app.locale, "Not chosen: files open from WhatsAwra's cache").into_owned(),
            |folder| folder.display().to_string(),
        );
        self.folder.set_subtitle(Some(folder.as_str()));
        self.start_at_login
            .set_visible(app.start_with_system.is_some());
    }
}

/// A titled card of rows, appended to `column`.
fn section(column: &awra::Box, title: &str) -> awra::Section {
    let section = awra::Section::new();
    section.set_title(Some(title));
    column.append(&section);
    section
}

/// A row with a title, a quieter description and a control at its end.
fn row(title: &str, description: &str, control: &impl awra::IsWidget) -> awra::ActionRow {
    let row = awra::ActionRow::new(title, description);
    control.upcast_ref().set_valign(awra::Align::Center);
    row.set_suffix(Some(control));
    row
}

/// A row whose button queues `action`.
fn button_row(
    driver: &Rc<Driver>,
    title: &str,
    description: &str,
    label: &str,
    action: Action,
) -> awra::ActionRow {
    let button = awra::Button::with_label(label);
    let weak = Rc::downgrade(driver);
    button.connect_clicked(move |_| {
        if let Some(driver) = weak.upgrade() {
            driver.act(action.clone());
        }
    });
    row(title, description, &button)
}

/// A switch row bound to a setting: `get` reads it at each turn, `set`
/// writes it on the next turn after a click.
fn toggle(
    driver: &Rc<Driver>,
    section: &awra::Section,
    title: &str,
    description: &str,
    get: fn(&App) -> bool,
    set: fn(&mut App, bool),
) -> Toggle {
    let switch = awra::Switch::new();
    let weak = Rc::downgrade(driver);
    switch.connect_active_notify(move |switch| {
        let on = switch.is_active();
        if let Some(driver) = weak.upgrade() {
            driver.edit(move |app| {
                if get(app) != on {
                    set(app, on);
                }
            });
        }
    });
    section.append(&row(title, description, &switch));
    Toggle { switch, get }
}
