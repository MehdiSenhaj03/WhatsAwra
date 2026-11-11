//! The window: Awra's header over a rail of places, the chat list and the
//! open chat, with the linking screen in their place until an account is
//! linked.

use std::rc::Rc;

use awra::prelude::*;

use super::Driver;
use super::chat_list::ChatList;
use super::conversation::ConversationPane;
use super::login::Login;
use crate::app::App;
use crate::i18n::gettext;
use crate::model::{Action, Page};

pub struct Shell {
    pub window: awra::Window,
    /// The language the views were made in.
    pub locale: crate::i18n::Locale,
    toasts: awra::ToastOverlay,
    pages: awra::Stack,
    login: Login,
    rail: Rail,
    chats: ChatList,
    conversation: ConversationPane,
    /// What the area beside the rail shows: the chats or a page.
    places: awra::Stack,
    settings: super::settings::SettingsPage,
    /// The theme last given to Awra.
    theme: Option<crate::settings::ThemeChoice>,
    dialogs: super::dialogs::Dialogs,
    /// Toasts already shown, by their place in `App::toasts` and text.
    shown_toasts: Vec<String>,
}

/// The places down the left edge (the egui window's navigation rail).
struct Rail {
    rail: awra::Rail,
    chats: awra::RailItem,
    calls: awra::RailItem,
    starred: awra::RailItem,
    archived: awra::RailItem,
    settings: awra::RailItem,
    account: awra::RailItem,
    account_avatar: awra::Avatar,
}

impl Rail {
    fn build(driver: &Rc<Driver>, locale: crate::i18n::Locale) -> Self {
        let rail = awra::Rail::new();
        let item = |label: &'static str, icon: &str| {
            let item = awra::RailItem::new(Some(gettext(locale, label).as_ref()));
            item.set_icon_name(Some(icon));
            item
        };
        let chats = item("Chats", "awra-chats-symbolic");
        let calls = item("Calls", "awra-call-symbolic");
        let starred = item("Starred", "starred-symbolic");
        let archived = item("Archived", "awra-archive-symbolic");
        let settings = item("Settings", "emblem-system-symbolic");
        let account = awra::RailItem::new(Some(gettext(locale, "Accounts").as_ref()));
        let account_avatar = awra::Avatar::new(32, None);
        account.set_child(Some(&account_avatar));
        let header = awra::Box::new(awra::Orientation::Vertical, 4);
        header.append(&chats);
        header.append(&calls);
        rail.set_header(Some(&header));
        rail.append(&starred);
        rail.append(&archived);
        let footer = awra::Box::new(awra::Orientation::Vertical, 4);
        footer.append(&settings);
        footer.append(&account);
        rail.set_footer(Some(&footer));

        let weak = Rc::downgrade(driver);
        let on = move |action: fn() -> Action| {
            let weak = weak.clone();
            move |_: &awra::RailItem| {
                if let Some(driver) = weak.upgrade() {
                    driver.act(action());
                }
            }
        };
        chats.connect_activate(on(|| Action::Open(Page::Chats)));
        calls.connect_activate(on(|| Action::Open(Page::Calls)));
        starred.connect_activate(on(|| Action::ToggleStarred));
        archived.connect_activate(on(|| Action::ShowArchived(true)));
        settings.connect_activate(on(|| Action::Open(Page::Settings)));
        Self {
            rail,
            chats,
            calls,
            starred,
            archived,
            settings,
            account,
            account_avatar,
        }
    }

    fn sync(&self, app: &App) {
        let place = if app.page == Page::Settings {
            &self.settings
        } else if app.page == Page::Calls {
            &self.calls
        } else if app.show_starred {
            &self.starred
        } else if app.show_archived {
            &self.archived
        } else {
            &self.chats
        };
        if !place.is_selected() {
            self.rail.set_selected_item(Some(place));
        }
        self.chats.set_unread(app.unread_chat_count() > 0);
        self.archived.set_visible(app.archived_count() > 0);
        self.archived.set_unread(app.archived_unread() > 0);
        self.starred
            .set_visible(!app.starred.is_empty() || app.show_starred);
        let title = app.self_title();
        self.account_avatar.set_text(Some(&title));
        self.account.set_label(Some(&title));
        self.account
            .set_unread(app.unread_chat_count_elsewhere() > 0);
        let me = app.me.clone();
        let picture = me
            .as_deref()
            .and_then(|me| app.avatar_path(me))
            .and_then(|path| super::pictures::file(&path));
        self.account_avatar.set_custom_image(picture.as_ref());
    }
}

impl Shell {
    pub fn build(driver: &Rc<Driver>, window: awra::Window) -> Self {
        let locale = driver.read(|app| app.locale);
        let header = awra::Header::new();
        header.set_title(Some("WhatsAwra"));
        window.set_titlebar(Some(&header));

        let rail = Rail::build(driver, locale);
        let chats = ChatList::build(driver, locale);
        let conversation = ConversationPane::build(driver, locale);
        let login = Login::build(driver, locale);

        let split = awra::SplitView::new();
        split.set_sidebar(Some(&chats.root));
        split.set_sidebar_width(340);
        split.set_content(Some(&conversation.root));

        let settings = driver.read(|app| super::settings::SettingsPage::build(driver, app));
        let places = awra::Stack::new();
        places.set_transition_type(awra::StackTransitionType::Crossfade);
        places.set_hexpand(true);
        places.add_named(&split, Some("chats"));
        places.add_named(&settings.root, Some("settings"));

        let main = awra::Box::new(awra::Orientation::Horizontal, 0);
        main.append(&rail.rail);
        main.append(&places);

        let pages = awra::Stack::new();
        pages.set_transition_type(awra::StackTransitionType::Crossfade);
        pages.add_named(&login.root, Some("login"));
        pages.add_named(&main, Some("main"));

        let toasts = awra::ToastOverlay::new();
        toasts.set_child(Some(&pages));
        window.set_child(Some(&toasts));

        Self {
            window,
            locale,
            toasts,
            pages,
            login,
            rail,
            chats,
            conversation,
            places,
            settings,
            theme: None,
            dialogs: super::dialogs::Dialogs::new(),
            shown_toasts: Vec::new(),
        }
    }

    pub fn sync(&mut self, driver: &Rc<Driver>, app: &App) {
        let page = if app.is_linked() { "main" } else { "login" };
        if self.pages.visible_child_name().as_deref() != Some(page) {
            self.pages.set_visible_child_name(page);
        }
        self.sync_theme(app);
        if app.is_linked() {
            self.rail.sync(app);
            let place = match app.page {
                Page::Settings => "settings",
                _ => "chats",
            };
            if self.places.visible_child_name().as_deref() != Some(place) {
                self.places.set_visible_child_name(place);
            }
            if place == "settings" {
                self.settings.sync(app);
            }
            self.chats.sync(driver, app);
            self.conversation.sync(driver, app);
        } else {
            self.login.sync(app);
        }
        self.dialogs.sync(driver, app, &self.window);
        self.sync_toasts(driver, app);
    }

    /// Awra's appearance follows the theme chosen in Settings; Messages,
    /// a macOS look, follows the system here.
    fn sync_theme(&mut self, app: &App) {
        use crate::settings::ThemeChoice;
        if self.theme == Some(app.settings.theme) {
            return;
        }
        self.theme = Some(app.settings.theme);
        let appearance = match app.settings.theme {
            ThemeChoice::Light => awra::Appearance::Light,
            ThemeChoice::Dark => awra::Appearance::Dark,
            ThemeChoice::System | ThemeChoice::Messages => awra::Appearance::System,
        };
        // A shot asked for one appearance keeps it.
        if std::env::var_os("AWRA_APPEARANCE").is_none() {
            awra::StyleManager::default().set_appearance(appearance);
        }
    }

    /// Opens a context menu as a right click would (shots of menus).
    pub fn open_menu(&self, driver: &Rc<Driver>, which: &str) {
        match which {
            "chat" => self.chats.open_menu(driver, 0),
            _ => self.conversation.open_menu(driver),
        }
    }

    /// Shows each toast the app adds once; the app keeps its own list and
    /// dismisses on its own clock.
    fn sync_toasts(&mut self, driver: &Rc<Driver>, app: &App) {
        let current: Vec<String> = app
            .toasts
            .iter()
            .map(|toast| toast.message.clone())
            .collect();
        for (index, text) in current.iter().enumerate() {
            if self.shown_toasts.contains(text) {
                continue;
            }
            let weak = Rc::downgrade(driver);
            let timeout = match app.toasts[index].kind {
                crate::model::ToastKind::Info => 4000,
                _ => 0,
            };
            let dismiss = gettext(app.locale, "Dismiss");
            self.toasts
                .show_with_callback(text, dismiss.as_ref(), timeout, move || {
                    if let Some(driver) = weak.upgrade() {
                        driver.act(Action::DismissToast(index));
                    }
                });
        }
        self.shown_toasts = current;
    }
}
