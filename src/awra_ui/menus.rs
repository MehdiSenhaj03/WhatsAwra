//! The context menus: a message's and a chat's, built from the app as it is
//! when the menu opens. Each item queues the `Action` its egui counterpart
//! pushes, through the same helpers.

use std::rc::Rc;

use awra::menu::Entry;

use super::Driver;
use crate::app::App;
use crate::i18n::gettext;
use crate::model::{Action, Content, Dialog};
use crate::ui::conversation::{
    can_reply, copyable_text, delete_for_everyone_action, delete_for_me_action, forward_action,
};

/// The reactions offered at once, as the phone's quick row.
pub const QUICK_REACTIONS: [&str; 6] = ["👍", "❤️", "😂", "😮", "😢", "🙏"];

/// An item that queues `action`.
fn item(driver: &Rc<Driver>, label: impl Into<String>, action: Action) -> Entry {
    let weak = Rc::downgrade(driver);
    Entry::item(label, move || {
        if let Some(driver) = weak.upgrade() {
            driver.act(action.clone());
        }
    })
}

/// The menu of message `id` in the open chat.
pub fn message(driver: &Rc<Driver>, app: &App, id: &str) -> Vec<Entry> {
    let locale = app.locale;
    let Some(chat) = app.current_chat() else {
        return Vec::new();
    };
    let Some(message) = app
        .conversations
        .get(&chat.id)
        .and_then(|conversation| conversation.message(id))
    else {
        return Vec::new();
    };
    let chat_id = chat.id.clone();
    let mut entries = Vec::new();
    let revoked = matches!(message.content, Content::Revoked);

    if can_reply(&message.content) && chat.can_send() {
        entries.push(
            item(driver, gettext(locale, "Reply"), Action::Reply(id.to_owned()))
                .icon("awra-reply-symbolic"),
        );
    }
    if chat.is_group() && !message.from_me && can_reply(&message.content) {
        entries.push(item(
            driver,
            gettext(locale, "Reply privately"),
            Action::ReplyPrivately {
                chat: chat_id.clone(),
                message: id.to_owned(),
            },
        ));
    }
    if !revoked && chat.can_send() {
        let reactions = QUICK_REACTIONS
            .iter()
            .map(|emoji| {
                item(
                    driver,
                    *emoji,
                    Action::React {
                        chat: chat_id.clone(),
                        message: id.to_owned(),
                        emoji: (*emoji).to_owned(),
                    },
                )
            })
            .collect();
        entries.push(Entry::submenu(gettext(locale, "React"), reactions).icon("face-smile-symbolic"));
    }
    if let Some(text) = copyable_text(&message.content) {
        let plain = crate::markup::plain(&text, &app.mention_list(message));
        entries.push(
            item(driver, gettext(locale, "Copy text"), Action::CopyText(plain))
                .icon("edit-copy-symbolic"),
        );
    }
    if crate::app::can_select(&message.content) {
        entries.push(
            item(driver, gettext(locale, "Forward"), forward_action(&chat_id, id))
                .icon("awra-forward-symbolic"),
        );
    }
    if !revoked {
        let starred = app
            .stars
            .get(&chat_id)
            .is_some_and(|stars| stars.contains(id));
        entries.push(
            item(
                driver,
                gettext(locale, if starred { "Unstar" } else { "Star" }),
                Action::SetStar {
                    chat: chat_id.clone(),
                    message: id.to_owned(),
                    starred: !starred,
                },
            )
            .icon(if starred {
                "non-starred-symbolic"
            } else {
                "starred-symbolic"
            }),
        );
        let pinned = app
            .pins
            .get(&chat_id)
            .is_some_and(|pins| pins.contains(id));
        entries.push(
            item(
                driver,
                gettext(locale, if pinned { "Unpin message" } else { "Pin message" }),
                Action::SetMessagePinned {
                    chat: chat_id.clone(),
                    message: id.to_owned(),
                    pinned: !pinned,
                },
            )
            .icon("view-pin-symbolic"),
        );
    }
    if crate::app::can_select(&message.content) {
        entries.push(
            item(driver, gettext(locale, "Select"), Action::SelectMessage(id.to_owned()))
                .icon("object-select-symbolic"),
        );
    }
    if let Some(path) = message.content.media().and_then(|media| media.path.clone()) {
        entries.push(Entry::separator());
        entries.push(
            item(driver, gettext(locale, "Open"), Action::OpenFile(path.clone()))
                .icon("document-open-symbolic"),
        );
        if let Some(folder) = path.parent() {
            entries.push(
                item(
                    driver,
                    gettext(locale, "Show in folder"),
                    Action::OpenFolder(folder.to_path_buf()),
                )
                .icon("folder-open-symbolic"),
            );
        }
    }
    entries.push(Entry::separator());
    if message.from_me && !revoked {
        entries.push(
            item(
                driver,
                gettext(locale, "Message info"),
                Action::ShowDialog(Dialog::MessageInfo {
                    chat: chat_id.clone(),
                    message: id.to_owned(),
                }),
            )
            .icon("info-outline-symbolic"),
        );
    }
    if app.can_edit(message) {
        entries.push(
            item(driver, gettext(locale, "Edit"), Action::Edit(id.to_owned()))
                .icon("document-edit-symbolic"),
        );
    }
    if app.can_revoke(message) {
        entries.push(
            item(
                driver,
                gettext(locale, "Delete for everyone"),
                delete_for_everyone_action(&chat_id, id),
            )
            .icon("user-trash-symbolic")
            .destructive(),
        );
    }
    entries.push(
        item(
            driver,
            gettext(locale, "Delete for me"),
            delete_for_me_action(&chat_id, id),
        )
        .icon("user-trash-symbolic")
        .destructive(),
    );
    entries
}

/// The menu of a chat in the list.
pub fn chat(driver: &Rc<Driver>, app: &App, id: &str) -> Vec<Entry> {
    let locale = app.locale;
    let Some(chat) = app.chat(id) else {
        return Vec::new();
    };
    let id = chat.id.clone();
    let now = crate::util::now();
    let mut entries = Vec::new();
    entries.push(
        item(
            driver,
            gettext(locale, if chat.pinned { "Unpin" } else { "Pin to top" }),
            Action::SetPinned(id.clone(), !chat.pinned),
        )
        .icon("view-pin-symbolic"),
    );
    if !chat.is_channel() {
        entries.push(
            item(
                driver,
                gettext(
                    locale,
                    if chat.favorite {
                        "Remove from favorites"
                    } else {
                        "Add to favorites"
                    },
                ),
                Action::SetFavorite(id.clone(), !chat.favorite),
            )
            .icon("starred-symbolic"),
        );
    }
    entries.push(
        item(
            driver,
            gettext(locale, if chat.archived { "Unarchive" } else { "Archive" }),
            Action::SetArchived(id.clone(), !chat.archived),
        )
        .icon("awra-archive-symbolic"),
    );
    let muted = chat.muted(now);
    entries.push(
        item(
            driver,
            gettext(locale, if muted { "Unmute" } else { "Mute" }),
            Action::SetMuted(id.clone(), if muted { None } else { Some(0) }),
        )
        .icon(if muted {
            "audio-volume-high-symbolic"
        } else {
            "audio-volume-muted-symbolic"
        }),
    );
    entries.push(if chat.looks_unread() {
        item(driver, gettext(locale, "Mark as read"), Action::MarkRead(id.clone()))
            .icon("check-symbolic")
    } else {
        item(driver, gettext(locale, "Mark as unread"), Action::MarkUnread(id.clone()))
            .icon("awra-chats-symbolic")
    });
    entries.push(Entry::separator());
    entries.push(
        item(
            driver,
            gettext(locale, "Clear chat"),
            Action::ShowDialog(Dialog::ConfirmClearChat(id.clone())),
        )
        .icon("edit-clear-all-symbolic"),
    );
    entries.push(
        item(
            driver,
            gettext(locale, "Delete chat"),
            Action::ShowDialog(Dialog::ConfirmDeleteChat(id)),
        )
        .icon("user-trash-symbolic")
        .destructive(),
    );
    entries
}
