//! The window: Awra's header over a rail of places, the chat list and the
//! open chat, with the linking screen in their place until an account is
//! linked.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use awra::prelude::*;

use super::Driver;
use super::chat_list::ChatList;
use super::conversation::ConversationPane;
use super::login::Login;
use crate::app::App;
use crate::i18n::gettext;
use crate::model::{Action, Page};

/// Pictures read from disk once and shared by every view that shows them.
#[derive(Default)]
pub struct Pictures {
    textures: HashMap<PathBuf, Option<awra::Texture>>,
}

impl Pictures {
    pub fn get(&mut self, path: &Path) -> Option<awra::Texture> {
        self.textures
            .entry(path.to_path_buf())
            .or_insert_with(|| awra::Texture::from_filename(path).ok())
            .clone()
    }
}

pub struct Shell {
    pub window: awra::Window,
    toasts: awra::ToastOverlay,
    pages: awra::Stack,
    login: Login,
    rail: Rail,
    chats: ChatList,
    conversation: ConversationPane,
    pictures: Pictures,
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

    fn sync(&self, app: &App, pictures: &mut Pictures) {
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
            .and_then(|path| pictures.get(&path));
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

        let main = awra::Box::new(awra::Orientation::Horizontal, 0);
        main.append(&rail.rail);
        split.set_hexpand(true);
        main.append(&split);

        let pages = awra::Stack::new();
        pages.set_transition_type(awra::StackTransitionType::Crossfade);
        pages.add_named(&login.root, Some("login"));
        pages.add_named(&main, Some("main"));

        let toasts = awra::ToastOverlay::new();
        toasts.set_child(Some(&pages));
        window.set_child(Some(&toasts));

        Self {
            window,
            toasts,
            pages,
            login,
            rail,
            chats,
            conversation,
            pictures: Pictures::default(),
            shown_toasts: Vec::new(),
        }
    }

    pub fn sync(&mut self, driver: &Rc<Driver>, app: &App) {
        let page = if app.is_linked() { "main" } else { "login" };
        if self.pages.visible_child_name().as_deref() != Some(page) {
            self.pages.set_visible_child_name(page);
        }
        if app.is_linked() {
            self.rail.sync(app, &mut self.pictures);
            self.chats.sync(driver, app, &mut self.pictures);
            self.conversation.sync(driver, app, &mut self.pictures);
        } else {
            self.login.sync(app);
        }
        self.sync_toasts(driver, app);
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
