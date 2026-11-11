//! The window: Awra's header over a rail of places, the chat list and the
//! open chat, with the linking screen in their place until an account is
//! linked.

use std::rc::Rc;

use awra::prelude::*;

use super::Driver;
use super::chat_list::ChatList;
use super::conversation::ConversationPane;
use super::login::Login;
use whatsawra::app::App;
use whatsawra::i18n::gettext;
use whatsawra::model::{Action, Page};

/// The room between the rail's places, the rail's own between its entries.
const RAIL_GAP: i32 = 8;
/// The rail's width and the list's, which the window's header follows.
const RAIL_WIDTH: i32 = 72;
const LIST_WIDTH: i32 = 340;
/// The header's own room before its first widget.
const HEADER_INSET: i32 = 14;
const HEADER_START_WIDTH: i32 = RAIL_WIDTH + LIST_WIDTH - HEADER_INSET;

pub struct Shell {
    pub window: awra::Window,
    /// The language the views were made in.
    pub locale: whatsawra::i18n::Locale,
    toasts: awra::ToastOverlay,
    pages: awra::Stack,
    login: Login,
    lock: super::lock::LockScreen,
    call: super::call::CallScreen,
    viewer: super::viewer::Viewer,
    rail: Rail,
    chats: ChatList,
    conversation: ConversationPane,
    /// What the area beside the rail shows: the chats or a page.
    places: awra::Stack,
    call_banner: awra::StatusBanner,
    settings: super::settings::SettingsPage,
    /// The list beside the open chat: the chats, or the call history.
    lists: awra::Stack,
    calls: super::calls::CallList,
    split: awra::SplitView,
    info: super::info::InfoPane,
    info_scroll: awra::ScrolledWindow,
    /// What the end pane shows: "info", "search" or nothing.
    end: Option<&'static str>,
    search: super::search::SearchPane,
    /// The theme last given to Awra.
    theme: Option<whatsawra::settings::ThemeChoice>,
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
    /// Who is linked: our face in the window's top-left corner, over
    /// the rail (the shell puts it in the header).
    pub account: awra::Button,
    account_avatar: awra::Avatar,
}

impl Rail {
    fn build(driver: &Rc<Driver>, locale: whatsawra::i18n::Locale) -> Self {
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
        let account_avatar = awra::Avatar::new(30, None);
        let account = awra::Button::new();
        account.set_child(Some(&account_avatar));
        account.set_appearance(awra::ButtonAppearance::Toolbar);
        account.set_halign(awra::Align::Center);
        account.set_valign(awra::Align::Center);
        let header = awra::Box::new(awra::Orientation::Vertical, RAIL_GAP);
        header.append(&chats);
        header.append(&calls);
        header.set_margin_top(RAIL_GAP);
        rail.set_header(Some(&header));
        rail.append(&starred);
        rail.append(&archived);
        let footer = awra::Box::new(awra::Orientation::Vertical, RAIL_GAP);
        footer.append(&settings);
        footer.set_margin_bottom(RAIL_GAP);
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
        // The account's place is in Settings until the account switcher.
        let weak = Rc::downgrade(driver);
        account.connect_clicked(move |_| {
            if let Some(driver) = weak.upgrade() {
                driver.act(Action::Open(Page::Settings));
            }
        });
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
        self.account.set_tooltip_text(Some(title.as_str()));
        let me = app.me.clone();
        let picture = me.as_deref().and_then(|me| app.avatar_path(me));
        super::pictures::avatar(&self.account_avatar, picture.as_deref());
    }
}

impl Shell {
    pub fn build(driver: &Rc<Driver>, window: awra::Window) -> Self {
        let locale = driver.read(|app| app.locale);
        let rail = Rail::build(driver, locale);
        let chats = ChatList::build(driver, locale);
        let conversation = ConversationPane::build(driver, locale);

        // The window's header carries no name of its own: over the rail and
        // the list, the list's heading; over the chat, the chat's header, up
        // to the window's buttons. It is the top of the panes below it.
        let header = awra::Header::new();
        header.set_expand_center(true);
        header.set_compact(true);
        let start = awra::Box::new(awra::Orientation::Horizontal, 0);
        start.set_size_request(HEADER_START_WIDTH, -1);
        // Our face over the rail's column, centred on it: the corner names
        // who is linked instead of standing empty.
        let corner = awra::Box::new(awra::Orientation::Horizontal, 0);
        corner.set_size_request(2 * (RAIL_WIDTH / 2 - HEADER_INSET), -1);
        corner.append(&rail.account);
        start.append(&corner);
        chats.heading.set_hexpand(true);
        chats.heading.set_valign(awra::Align::Center);
        // The heading starts where the list's rows do.
        chats
            .heading
            .set_margin_start(RAIL_WIDTH + 16 - HEADER_INSET - 2 * (RAIL_WIDTH / 2 - HEADER_INSET));
        chats.heading.set_margin_end(8);
        start.append(&chats.heading);
        header.set_start_widget(Some(&start));
        header.set_center_widget(Some(&conversation.header));
        window.set_titlebar(Some(&header));
        let login = Login::build(driver, locale);

        let split = awra::SplitView::new();
        let calls = super::calls::CallList::build(driver, locale);
        let lists = awra::Stack::new();
        lists.add_named(&chats.root, Some("chats"));
        lists.add_named(&calls.root, Some("calls"));
        split.set_sidebar(Some(&lists));
        split.set_sidebar_width(LIST_WIDTH);
        split.set_content(Some(&conversation.root));
        let info = super::info::InfoPane::build(driver, locale);
        let info_scroll = awra::ScrolledWindow::new();
        info_scroll.set_hscrollbar_policy(awra::PolicyType::Never);
        info_scroll.set_child(Some(&info.root));
        split.set_end_pane_width(340);
        let search = super::search::SearchPane::build(driver, locale);

        let settings =
            driver.read(|app| super::settings::SettingsPage::build(driver, app, &window));
        let places = awra::Stack::new();
        places.set_transition_type(awra::StackTransitionType::Crossfade);
        places.set_hexpand(true);
        places.set_vexpand(true);
        places.add_named(&split, Some("chats"));
        places.add_named(&settings.root, Some("settings"));

        let main = awra::Box::new(awra::Orientation::Horizontal, 0);
        main.append(&rail.rail);
        // A call left for the chats: the way back to it, above them.
        let call_banner = awra::StatusBanner::new();
        call_banner.set_appearance(awra::StatusAppearance::Success);
        call_banner.set_dismissible(false);
        let back = awra::Button::with_label(gettext(locale, "Return to the call").as_ref());
        let weak = Rc::downgrade(driver);
        back.connect_clicked(move |_| {
            if let Some(driver) = weak.upgrade() {
                driver.act(Action::ReturnToCall);
            }
        });
        call_banner.set_action(Some(&back));
        let beside = awra::Box::new(awra::Orientation::Vertical, 0);
        beside.set_hexpand(true);
        beside.append(&call_banner);
        beside.append(&places);
        main.append(&beside);

        let pages = awra::Stack::new();
        pages.set_transition_type(awra::StackTransitionType::Crossfade);
        pages.add_named(&login.root, Some("login"));
        pages.add_named(&main, Some("main"));
        let lock = super::lock::LockScreen::build(driver, locale);
        pages.add_named(&lock.root, Some("lock"));
        let call = super::call::CallScreen::build(driver, locale);
        pages.add_named(&call.root, Some("call"));
        let viewer = super::viewer::Viewer::build(driver, locale);
        pages.add_named(&viewer.root, Some("viewer"));

        let toasts = awra::ToastOverlay::new();
        toasts.set_child(Some(&pages));
        window.set_child(Some(&toasts));

        Self {
            window,
            locale,
            toasts,
            pages,
            login,
            lock,
            call,
            viewer,
            rail,
            chats,
            conversation,
            places,
            call_banner,
            settings,
            lists,
            calls,
            split,
            info,
            info_scroll,
            end: None,
            search,
            theme: None,
            dialogs: super::dialogs::Dialogs::new(),
            shown_toasts: Vec::new(),
        }
    }

    pub fn sync(&mut self, driver: &Rc<Driver>, app: &App) {
        let locked = app.app_lock.is_locked();
        let calling = !locked && self.call.sync(app);
        let viewing = !locked && !calling && self.viewer.sync(app);
        let page = if locked {
            "lock"
        } else if calling {
            "call"
        } else if viewing {
            "viewer"
        } else if app.is_linked() {
            "main"
        } else {
            "login"
        };
        if self.pages.visible_child_name().as_deref() != Some(page) {
            self.pages.set_visible_child_name(page);
            if page == "viewer" {
                self.viewer.focus();
            }
        }
        self.sync_theme(app);
        if locked {
            // Nothing but the lock screen while locked: no dialog, no toast.
            self.lock.sync(app);
            self.dialogs.close();
            return;
        }
        if app.is_linked() {
            self.rail.sync(app);
            let hidden_call = app.call.as_ref().filter(|_| app.call_surface_hidden);
            self.call_banner.set_revealed(hidden_call.is_some());
            if let Some(call) = hidden_call {
                self.call_banner
                    .set_title(Some(app.chat_title_of(&call.chat).as_str()));
                self.call_banner
                    .set_description(Some(whatsawra::ui::call::status(app.locale, call).as_str()));
            }
            self.chats.sync_heading(app);
            let place = match app.page {
                Page::Settings => "settings",
                _ => "chats",
            };
            if self.places.visible_child_name().as_deref() != Some(place) {
                self.places.set_visible_child_name(place);
            }
            if place == "settings" {
                super::profile::span("settings", || self.settings.sync(app));
            }
            let list = if app.page == Page::Calls {
                "calls"
            } else {
                "chats"
            };
            if self.lists.visible_child_name().as_deref() != Some(list) {
                self.lists.set_visible_child_name(list);
            }
            if list == "calls" {
                super::profile::span("calls", || self.calls.sync(driver, app));
            } else {
                super::profile::span("chat list", || self.chats.sync(driver, app));
            }
            super::profile::span("conversation", || self.conversation.sync(driver, app));
            // The info panel and the search pane share the place beside the chat.
            let info = super::profile::span("info", || self.info.sync(driver, app));
            let searching = super::profile::span("search", || self.search.sync(app));
            let end = if searching {
                Some("search")
            } else if info {
                Some("info")
            } else {
                None
            };
            if end != self.end {
                match end {
                    Some("search") => {
                        self.split.set_end_pane(Some(&self.search.root));
                        self.search.focus();
                    }
                    Some(_) => self.split.set_end_pane(Some(&self.info_scroll)),
                    None => self.split.set_end_pane(None::<&awra::Widget>),
                }
                self.end = end;
            }
        } else {
            self.login.sync(app);
        }
        super::profile::span("dialogs", || self.dialogs.sync(driver, app, &self.window));
        self.sync_toasts(driver, app);
    }

    /// Awra's appearance follows the theme chosen in Settings; Messages,
    /// a macOS look, follows the system here.
    fn sync_theme(&mut self, app: &App) {
        use whatsawra::settings::ThemeChoice;
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

    /// Where the keyboard starts: the open chat's composer, else the search.
    pub fn focus_start(&self) {
        self.chats.focus_search();
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
                whatsawra::model::ToastKind::Info => 4000,
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
