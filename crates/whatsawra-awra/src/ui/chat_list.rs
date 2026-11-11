//! The chat list: a search field, the filter chips and a virtualized list
//! of chats, each row a face, the chat's name, its latest message, when it
//! came and how much is unread.

use std::cell::RefCell;
use std::collections::HashSet;
use std::path::PathBuf;
use std::rc::Rc;

use awra::prelude::*;

use super::{Driver, Op};
use whatsawra::app::App;
use whatsawra::i18n::{Locale, gettext};
use whatsawra::model::{Action, Chat, ChatFilter, Delivery};

/// What one row shows, read from the app at each turn.
#[derive(Clone, Debug, PartialEq)]
struct Row {
    id: String,
    title: String,
    preview: String,
    stamp: String,
    unread: u32,
    marked_unread: bool,
    mention: bool,
    muted: bool,
    pinned: bool,
    group: bool,
    avatar: Option<PathBuf>,
    open: bool,
    /// The tick of our own latest message.
    status: Option<Delivery>,
}

/// The widgets of one row, kept on the row's root.
#[derive(Clone)]
struct RowWidgets {
    button: awra::ToggleButton,
    avatar: awra::Avatar,
    title: awra::Label,
    stamp: awra::Label,
    preview: awra::Label,
    pin: awra::Image,
    mute: awra::Image,
    unread: awra::Badge,
    shown: Rc<RefCell<Option<Row>>>,
    /// Pictures already asked for (shared by every row), so each is asked
    /// once, and only for rows on screen, as the egui list does.
    asked: Rc<RefCell<HashSet<String>>>,
    driver: std::rc::Weak<Driver>,
}

pub struct ChatList {
    pub root: awra::Box,
    /// The title and New chat, for the window's header.
    pub heading: awra::Box,
    new_chat: awra::Button,
    search: awra::SearchEntry,
    typed: super::Typed,
    chips: Vec<(ChatFilter, awra::Chip)>,
    store: awra::ListStore,
    list: awra::ListView,
    rows: Rc<RefCell<Vec<Row>>>,
    empty: awra::EmptyState,
    title: awra::Label,
}

impl ChatList {
    pub fn build(driver: &Rc<Driver>, locale: Locale) -> Self {
        let root = awra::Box::new(awra::Orientation::Vertical, 8);
        root.set_margin_top(8);

        // The list's heading sits in the window's header (the shell puts it
        // there), over the list.
        let heading = awra::Box::new(awra::Orientation::Horizontal, 8);
        let title = awra::Label::new(Some(gettext(locale, "Chats").as_ref()));
        title.set_typography(awra::Typography::Title3);
        title.set_xalign(0.0);
        title.set_hexpand(true);
        heading.append(&title);
        let new_chat = awra::Button::from_icon_name("document-edit-symbolic");
        new_chat.set_appearance(awra::ButtonAppearance::Ghost);
        new_chat.set_tooltip_text(Some(gettext(locale, "New chat").as_ref()));
        heading.append(&new_chat);

        let search = awra::SearchEntry::new();
        search.set_placeholder_text(Some(gettext(locale, "Search or start a new chat").as_ref()));
        search.set_margin_start(12);
        search.set_margin_end(12);
        root.append(&search);

        let chip_row = awra::Box::new(awra::Orientation::Horizontal, 6);
        chip_row.set_margin_start(12);
        chip_row.set_margin_end(12);
        let mut chips = Vec::new();
        for filter in [
            ChatFilter::All,
            ChatFilter::Unread,
            ChatFilter::Favorites,
            ChatFilter::Groups,
        ] {
            let chip = awra::Chip::new(Some(filter.label(locale).as_ref()));
            chip.set_toggleable(false);
            let weak = Rc::downgrade(driver);
            chip.connect_activate(move |_| {
                if let Some(driver) = weak.upgrade() {
                    driver.act(Action::SetChatFilter(filter));
                }
            });
            chip_row.append(&chip);
            chips.push((filter, chip));
        }
        root.append(&chip_row);

        let asked: Rc<RefCell<HashSet<String>>> = Rc::default();
        let shared = asked.clone();
        let rows: Rc<RefCell<Vec<Row>>> = Rc::default();
        let store = awra::ListStore::new::<awra::StringObject>();
        let factory = awra::SignalListItemFactory::new();
        let weak = Rc::downgrade(driver);
        factory.connect_setup(move |_, item| {
            let (root, widgets) = row_widgets(&weak, &shared);
            root.set_rust_data("row", widgets);
            item.set_child(Some(&root));
        });
        let bound = rows.clone();
        factory.connect_bind(move |_, item| {
            let Some(child) = item.child() else { return };
            let Some(widgets) = child.rust_data::<RowWidgets>("row") else {
                return;
            };
            let rows = bound.borrow();
            if let Some(row) = rows.get(item.position() as usize) {
                bind_row(&widgets, row);
            }
        });
        let list = awra::ListView::new(
            Some(awra::NoSelection::new(Some(store.clone()))),
            Some(factory),
        );
        list.set_single_click_activate(true);
        awra::add_style_role(&list, "people-list");
        let weak = Rc::downgrade(driver);
        let activated = rows.clone();
        list.connect_activate(move |_, position| {
            let id = activated
                .borrow()
                .get(position as usize)
                .map(|row| row.id.clone());
            if let (Some(driver), Some(id)) = (weak.upgrade(), id) {
                driver.act(Action::OpenChat(id));
            }
        });
        let scrolled = awra::ScrolledWindow::new();
        scrolled.set_child(Some(&list));
        scrolled.set_vexpand(true);

        let empty = awra::EmptyState::new();
        empty.set_icon_name(Some("awra-chats-symbolic"));
        empty.set_vexpand(true);
        empty.set_visible(false);

        root.append(&scrolled);
        root.append(&empty);

        let weak = Rc::downgrade(driver);
        let typed = super::Typed::default();
        let noted = typed.clone();
        search.connect_search_changed(move |entry| {
            let text = entry.text();
            noted.note(&text);
            if let Some(driver) = weak.upgrade() {
                driver.act(Action::Search(text));
            }
        });

        let weak = Rc::downgrade(driver);
        new_chat.connect_clicked(move |_| {
            if let Some(driver) = weak.upgrade() {
                driver.act(Action::ShowDialog(whatsawra::model::Dialog::NewChat));
            }
        });

        Self {
            root,
            heading,
            new_chat,
            search,
            typed,
            chips,
            store,
            list,
            rows,
            empty,
            title,
        }
    }

    /// The heading in the window's header names the place on screen, on
    /// every page.
    pub fn sync_heading(&self, app: &App) {
        let heading = match app.page {
            whatsawra::model::Page::Calls => gettext(app.locale, "Calls"),
            whatsawra::model::Page::Settings | whatsawra::model::Page::Wallpaper => {
                gettext(app.locale, "Settings")
            }
            _ if app.show_starred => gettext(app.locale, "Starred"),
            _ if app.show_archived => gettext(app.locale, "Archived"),
            _ => gettext(app.locale, "Chats"),
        };
        self.title.set_text(heading.as_ref());
        self.new_chat
            .set_visible(app.page == whatsawra::model::Page::Chats);
    }

    pub fn sync(&mut self, _driver: &Rc<Driver>, app: &App) {
        self.typed.follow(&self.search, &app.search);
        for (filter, chip) in &self.chips {
            chip.set_active(app.chat_filter == *filter && app.label_filter.is_none());
        }

        let chats = super::profile::span("list: visible_chats", || app.visible_chats());
        let rows: Vec<Row> = super::profile::span("list: rows", || {
            chats.iter().map(|chat| row_of(app, chat)).collect()
        });

        // The rows the list binds from become these; the old ones are only
        // compared against, never copied.
        let old = std::mem::replace(&mut *self.rows.borrow_mut(), rows);
        let rows = self.rows.borrow();
        let list = self.list.clone();
        super::rows::update(
            &self.store,
            &old,
            &rows,
            |row| row.id.as_str(),
            |position| list.rebind(position),
        );

        let nothing = rows.is_empty();
        self.empty.set_visible(nothing);
        if nothing {
            let (title, description) = if !app.search.is_empty() {
                ("No chats found", "")
            } else if app.chat_filter == ChatFilter::Unread {
                ("No unread chats", "")
            } else {
                (
                    "No chats yet",
                    "Chats from your phone appear here as they sync.",
                )
            };
            self.empty
                .set_title(Some(gettext(app.locale, title).as_ref()));
            self.empty.set_description(if description.is_empty() {
                None
            } else {
                Some(gettext(app.locale, description).into_owned())
            });
        }
    }
}

fn row_of(app: &App, chat: &Chat) -> Row {
    let locale = app.locale;
    let preview = if let Some(activity) = whatsawra::present::activity_preview(app, chat) {
        activity
    } else if let Some(draft) = app.draft_preview(&chat.id) {
        format!("{} {draft}", gettext(locale, "Draft:"))
    } else if let Some(last) = &chat.last {
        let summary = last.summary_in(locale);
        if last.from_me {
            format!("{}: {summary}", gettext(locale, "You"))
        } else if chat.is_group()
            && let Some(sender) = last.sender_name.as_deref()
        {
            format!("{}: {summary}", app.short_name(&last.sender, sender))
        } else {
            summary.into_owned()
        }
    } else {
        String::new()
    };
    Row {
        id: chat.id.clone(),
        title: app.chat_title(chat),
        preview: preview.replace('\n', " "),
        stamp: if chat.last_activity > 0 {
            whatsawra::util::chat_stamp(locale, chat.last_activity)
        } else {
            String::new()
        },
        unread: chat.unread,
        marked_unread: chat.marked_unread,
        mention: chat.unread_mention.is_some(),
        muted: chat.muted(whatsawra::util::now()),
        pinned: chat.pinned,
        group: chat.is_group(),
        avatar: app.avatar_path(&chat.id),
        open: app.open_chat.as_deref() == Some(chat.id.as_str()),
        status: chat
            .last
            .as_ref()
            .filter(|last| last.from_me)
            .map(|last| last.status),
    }
}

fn row_widgets(
    driver: &std::rc::Weak<Driver>,
    asked: &Rc<RefCell<HashSet<String>>>,
) -> (awra::ToggleButton, RowWidgets) {
    let content = awra::Box::new(awra::Orientation::Horizontal, 12);
    let avatar = awra::Avatar::new(44, None);
    avatar.set_valign(awra::Align::Center);
    content.append(&avatar);

    let lines = awra::Box::new(awra::Orientation::Vertical, 2);
    lines.set_hexpand(true);
    lines.set_valign(awra::Align::Center);

    let top = awra::Box::new(awra::Orientation::Horizontal, 6);
    let title = awra::Label::new(None);
    title.set_xalign(0.0);
    title.set_hexpand(true);
    title.set_ellipsize(awra::pango::EllipsizeMode::End);
    title.set_weight(600);
    let stamp = awra::Label::new(None);
    stamp.set_typography(awra::Typography::Muted);
    stamp.set_tabular_digits(true);
    top.append(&title);
    top.append(&stamp);

    let bottom = awra::Box::new(awra::Orientation::Horizontal, 6);
    let preview = awra::Label::new(None);
    preview.set_xalign(0.0);
    preview.set_hexpand(true);
    preview.set_ellipsize(awra::pango::EllipsizeMode::End);
    preview.set_typography(awra::Typography::Muted);
    let pin = awra::Image::from_icon_name("view-pin-symbolic");
    pin.set_pixel_size(14);
    let mute = awra::Image::from_icon_name("audio-volume-muted-symbolic");
    mute.set_pixel_size(14);
    let unread = awra::Badge::new("");
    unread.set_appearance(awra::BadgeAppearance::Accent);
    bottom.append(&preview);
    bottom.append(&mute);
    bottom.append(&pin);
    bottom.append(&unread);

    lines.append(&top);
    lines.append(&bottom);
    content.append(&lines);
    // The whole row is a flat button: it lights under the pointer, presses,
    // and stays lit (checked) while its chat is open.
    let root = awra::ToggleButton::new();
    awra::add_style_role(&root, "row-action");
    root.set_child(Some(&content));
    root.set_margin_start(6);
    root.set_margin_end(6);
    let shown: Rc<RefCell<Option<Row>>> = Rc::default();
    // A click opens the chat, a right click its menu.
    let click = awra::GestureClick::new();
    click.set_button(3);
    let (weak, bound, anchor) = (driver.clone(), shown.clone(), root.clone());
    click.connect_pressed(move |_, _, x, y| {
        let id = bound.borrow().as_ref().map(|row| row.id.clone());
        let (Some(driver), Some(id)) = (weak.upgrade(), id) else {
            return;
        };
        let entries = driver.read(|app| super::menus::chat(&driver, app, &id));
        if !entries.is_empty() {
            awra::menu::popup_at(&anchor, x, y, entries);
        }
    });
    root.add_controller(click);
    let (weak, bound) = (driver.clone(), shown.clone());
    root.connect_clicked(move |button| {
        let id = bound
            .borrow()
            .as_ref()
            .map(|row| (row.id.clone(), row.open));
        if let (Some(driver), Some((id, open))) = (weak.upgrade(), id) {
            // The open chat stays checked; the next turn shows the new one.
            button.set_active(open);
            driver.act(Action::OpenChat(id));
        }
    });
    (
        root.clone(),
        RowWidgets {
            button: root,
            avatar,
            title,
            stamp,
            preview,
            pin,
            mute,
            unread,
            shown,
            asked: asked.clone(),
            driver: driver.clone(),
        },
    )
}

fn bind_row(widgets: &RowWidgets, row: &Row) {
    widgets.button.set_active(row.open);
    let mut shown = widgets.shown.borrow_mut();
    let picture_changed = shown
        .as_ref()
        .is_none_or(|old| old.avatar != row.avatar || old.id != row.id);
    widgets.avatar.set_text(Some(&row.title));
    widgets.avatar.set_shape(if row.group {
        awra::AvatarShape::Rounded
    } else {
        awra::AvatarShape::Circle
    });
    if picture_changed {
        super::pictures::avatar(&widgets.avatar, row.avatar.as_deref());
    }
    // A row coming on screen without a picture asks WhatsApp for it.
    if row.avatar.is_none()
        && widgets.asked.borrow_mut().insert(row.id.clone())
        && let Some(driver) = widgets.driver.upgrade()
    {
        let id = row.id.clone();
        driver.later(Op::Edit(Box::new(move |app: &mut App| {
            if app.avatar_unasked(&id) {
                app.avatar(&id);
            }
        })));
    }
    widgets.title.set_text(&row.title);
    widgets.stamp.set_text(&row.stamp);
    let tick = match row.status {
        Some(Delivery::Pending) => "🕓 ",
        Some(Delivery::Sent) => "✓ ",
        Some(Delivery::Delivered | Delivery::Read | Delivery::Played) => "✓✓ ",
        Some(Delivery::Failed) => "⚠ ",
        _ => "",
    };
    widgets.preview.set_text(&format!("{tick}{}", row.preview));
    widgets.pin.set_visible(row.pinned);
    widgets.mute.set_visible(row.muted);
    let badge = if row.mention {
        Some("@".to_owned())
    } else if row.unread > 0 {
        Some(if row.unread > 999 {
            "999+".to_owned()
        } else {
            row.unread.to_string()
        })
    } else if row.marked_unread {
        Some(String::new())
    } else {
        None
    };
    widgets.unread.set_visible(badge.is_some());
    if let Some(text) = badge {
        widgets.unread.set_label(&text);
        widgets.unread.set_appearance(if row.muted {
            awra::BadgeAppearance::Neutral
        } else {
            awra::BadgeAppearance::Accent
        });
    }
    if row.unread > 0 || row.marked_unread {
        widgets.stamp.set_typography(awra::Typography::Label);
    } else {
        widgets.stamp.set_typography(awra::Typography::Muted);
    }
    *shown = Some(row.clone());
}

impl ChatList {
    /// Opens the menu of the chat at `position`, as a right click would.
    pub fn open_menu(&self, driver: &Rc<Driver>, position: usize) {
        let Some(id) = self.rows.borrow().get(position).map(|row| row.id.clone()) else {
            return;
        };
        let entries = driver.read(|app| super::menus::chat(driver, app, &id));
        awra::menu::popup_at(&self.list, 120.0, 40.0, entries);
    }
}

impl ChatList {
    /// Puts the keyboard in the search field.
    pub fn focus_search(&self) {
        self.search.grab_focus();
    }
}
