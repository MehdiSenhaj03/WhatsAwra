//! The open chat's info, in the split view's end pane: who or what it is,
//! the chat's quick actions, a group's description and members, and the
//! group's management for those who may, and the actions that cannot be
//! taken back.

use std::rc::Rc;

use awra::prelude::*;

use super::Driver;
use whatsawra::app::App;
use whatsawra::i18n::{Locale, gettext};
use whatsawra::model::{Action, Chat, Dialog};

/// A chat and its members (id, name, admin), as the pane last showed them.
type Shown = (String, Vec<(String, String, bool)>);

pub struct InfoPane {
    pub root: awra::Box,
    heading: awra::Label,
    avatar: awra::Avatar,
    name: awra::Label,
    detail: awra::Label,
    mute: awra::Button,
    pin: awra::Button,
    archive: awra::Button,
    description_section: awra::Section,
    description: awra::Label,
    members_section: awra::Section,
    members: awra::Box,
    admin: super::group_admin::GroupAdmin,
    block: awra::Button,
    leave: awra::Button,
    shown: Option<Shown>,
}

/// The chat the pane shows, read when a button is pressed.
fn info_chat(driver: &Rc<Driver>) -> Option<String> {
    driver.read(|app| app.info.as_ref().map(|info| info.chat.clone()))
}

impl InfoPane {
    pub fn build(driver: &Rc<Driver>, locale: Locale) -> Self {
        let t = |text: &'static str| gettext(locale, text).into_owned();
        let root = awra::Box::new(awra::Orientation::Vertical, 16);
        root.set_margin_top(12);
        root.set_margin_start(16);
        root.set_margin_end(16);
        root.set_margin_bottom(16);
        root.set_size_request(320, -1);

        let top = awra::Box::new(awra::Orientation::Horizontal, 8);
        let heading = awra::Label::new(None);
        heading.set_typography(awra::Typography::Title3);
        heading.set_xalign(0.0);
        heading.set_hexpand(true);
        let close = awra::Button::from_icon_name("window-close-symbolic");
        close.set_appearance(awra::ButtonAppearance::Ghost);
        close.set_tooltip_text(Some(t("Close").as_str()));
        let weak = Rc::downgrade(driver);
        close.connect_clicked(move |_| {
            if let Some(driver) = weak.upgrade() {
                driver.act(Action::CloseInfo);
            }
        });
        top.append(&heading);
        top.append(&close);
        root.append(&top);

        let identity = awra::Box::new(awra::Orientation::Vertical, 6);
        let avatar = awra::Avatar::new(112, None);
        avatar.set_halign(awra::Align::Center);
        let name = awra::Label::new(None);
        name.set_typography(awra::Typography::Title2);
        name.set_wrap(true);
        name.set_justify(awra::Justification::Center);
        let detail = awra::Label::new(None);
        detail.set_typography(awra::Typography::Muted);
        detail.set_wrap(true);
        detail.set_justify(awra::Justification::Center);
        identity.append(&avatar);
        identity.append(&name);
        identity.append(&detail);
        root.append(&identity);

        // Quick actions, as the egui panel's row of buttons.
        let quick = awra::Box::new(awra::Orientation::Horizontal, 8);
        quick.set_halign(awra::Align::Center);
        let quick_button =
            |icon: &str, tooltip: &'static str, on: fn(String, &App) -> Vec<Action>| {
                let button = awra::Button::from_icon_name(icon);
                button.set_tooltip_text(Some(t(tooltip).as_str()));
                let weak = Rc::downgrade(driver);
                button.connect_clicked(move |_| {
                    let Some(driver) = weak.upgrade() else { return };
                    let Some(chat) = info_chat(&driver) else {
                        return;
                    };
                    for action in driver.read(|app| on(chat, app)) {
                        driver.act(action);
                    }
                });
                quick.append(&button);
                button
            };
        quick_button("awra-call-symbolic", "Voice call", |chat, _| {
            vec![Action::StartCall(chat)]
        });
        quick_button("awra-video-call-symbolic", "Video call", |chat, _| {
            vec![Action::StartVideoCall(chat)]
        });
        let mute = quick_button("audio-volume-muted-symbolic", "Mute", |chat, app| {
            let muted = app
                .chat(&chat)
                .is_some_and(|c| c.muted(whatsawra::util::now()));
            vec![Action::SetMuted(chat, if muted { None } else { Some(0) })]
        });
        let pin = quick_button("view-pin-symbolic", "Pin to top", |chat, app| {
            let pinned = app.chat(&chat).is_some_and(|c| c.pinned);
            vec![Action::SetPinned(chat, !pinned)]
        });
        let archive = quick_button("awra-archive-symbolic", "Archive", |chat, app| {
            let archived = app.chat(&chat).is_some_and(|c| c.archived);
            vec![Action::SetArchived(chat, !archived), Action::CloseInfo]
        });
        root.append(&quick);

        let description_section = awra::Section::new();
        description_section.set_title(Some(t("Description").as_str()));
        let description = awra::Label::new(None);
        description.set_wrap(true);
        description.set_xalign(0.0);
        description.set_selectable(true);
        description_section.append(&description);
        root.append(&description_section);

        let admin = super::group_admin::GroupAdmin::build(driver, locale);
        root.append(&admin.root);

        let members_section = awra::Section::new();
        let members = awra::Box::new(awra::Orientation::Vertical, 2);
        members_section.append(&members);
        root.append(&members_section);

        let danger = awra::Box::new(awra::Orientation::Vertical, 6);
        let danger_button = |label: String, dialog: fn(String, &App) -> Dialog| {
            let button = awra::Button::with_label(&label);
            button.set_appearance(awra::ButtonAppearance::Ghost);
            awra::add_style_role(&button, "quiet-destructive");
            let weak = Rc::downgrade(driver);
            button.connect_clicked(move |_| {
                let Some(driver) = weak.upgrade() else { return };
                if let Some(chat) = info_chat(&driver) {
                    let dialog = driver.read(|app| dialog(chat, app));
                    driver.act(Action::ShowDialog(dialog));
                }
            });
            danger.append(&button);
            button
        };
        let block = danger_button(t("Block"), |chat, app| Dialog::ConfirmBlock {
            blocked: !app.blocklist.contains(&chat),
            chat,
            from_list: false,
        });
        let leave = danger_button(t("Leave group"), |chat, _| Dialog::ConfirmLeaveGroup(chat));
        danger_button(t("Clear chat"), |chat, _| Dialog::ConfirmClearChat(chat));
        danger_button(t("Delete chat"), |chat, _| Dialog::ConfirmDeleteChat(chat));
        root.append(&danger);

        Self {
            root,
            heading,
            avatar,
            name,
            detail,
            mute,
            pin,
            archive,
            description_section,
            description,
            members_section,
            members,
            admin,
            block,
            leave,
            shown: None,
        }
    }

    /// Shows the info of `chat`; `None` when the panel is closed.
    pub fn sync(&mut self, driver: &Rc<Driver>, app: &App) -> bool {
        let Some(chat) = app.info.as_ref().and_then(|info| app.chat(&info.chat)) else {
            self.shown = None;
            return false;
        };
        let locale = app.locale;
        let t = |text: &'static str| gettext(locale, text).into_owned();
        let group = chat.is_group();
        self.heading.set_text(&if group {
            t("Group info")
        } else if chat.is_channel() {
            t("Channel info")
        } else {
            t("Contact info")
        });
        let title = app.chat_title(chat);
        self.avatar.set_text(Some(&title));
        self.avatar.set_shape(if group {
            awra::AvatarShape::Rounded
        } else {
            awra::AvatarShape::Circle
        });
        super::pictures::avatar(&self.avatar, app.avatar_path(&chat.id).as_deref());
        self.name.set_text(&title);
        self.detail.set_text(&detail_of(app, chat));

        let now = whatsawra::util::now();
        self.mute.set_icon_name(Some(if chat.muted(now) {
            "audio-volume-high-symbolic"
        } else {
            "audio-volume-muted-symbolic"
        }));
        self.mute.set_tooltip_text(Some(
            t(if chat.muted(now) { "Unmute" } else { "Mute" }).as_str(),
        ));
        self.pin.set_tooltip_text(Some(
            t(if chat.pinned { "Unpin" } else { "Pin to top" }).as_str(),
        ));
        self.archive.set_tooltip_text(Some(
            t(if chat.archived {
                "Unarchive"
            } else {
                "Archive"
            })
            .as_str(),
        ));

        let description = chat
            .group_description
            .as_deref()
            .filter(|text| !text.trim().is_empty());
        self.description_section.set_visible(description.is_some());
        if let Some(text) = description {
            self.description.set_text(text);
        }

        self.admin.sync(driver, app, chat);

        let blocked = app.blocklist.contains(&chat.id);
        self.block
            .set_label(Some(t(if blocked { "Unblock" } else { "Block" }).as_str()));
        self.block.set_visible(
            !group
                && !chat.is_channel()
                && whatsawra::blocking::eligible(
                    &chat.id,
                    app.me.as_deref(),
                    app.me_lid.as_deref(),
                ),
        );
        self.leave
            .set_visible(group && chat.can_leave(&app.our_ids()));

        let members: Vec<(String, String, bool)> = if group {
            app.participant_list(chat)
                .into_iter()
                .map(|(id, name)| {
                    let admin = chat.is_group_admin(&id);
                    (id, name, admin)
                })
                .collect()
        } else {
            Vec::new()
        };
        self.members_section.set_visible(!members.is_empty());
        let key = (chat.id.clone(), members.clone());
        if self.shown.as_ref() != Some(&key) {
            self.members_section.set_title(Some(
                whatsawra::i18n::ngettext(
                    locale,
                    "{count} member",
                    "{count} members",
                    members.len() as u32,
                )
                .replace("{count}", &members.len().to_string())
                .as_str(),
            ));
            while let Some(child) = self.members.first_child() {
                self.members.remove(&child);
            }
            for (id, name, admin) in &members {
                self.members
                    .append(&member_row(driver, app, chat, id, name, *admin));
            }
            self.shown = Some(key);
        }
        true
    }
}

/// The line under the name: a group's size, a person's number.
fn detail_of(app: &App, chat: &Chat) -> String {
    if chat.is_group() {
        let count = chat.participants.len();
        if count == 0 {
            return gettext(app.locale, "Group").into_owned();
        }
        return whatsawra::i18n::ngettext(
            app.locale,
            "Group · {count} member",
            "Group · {count} members",
            count as u32,
        )
        .replace("{count}", &count.to_string());
    }
    chat.phone().map(whatsawra::util::phone).unwrap_or_default()
}

/// One member: their face, name and whether they are an admin; a click
/// opens their chat.
fn member_row(
    driver: &Rc<Driver>,
    app: &App,
    chat: &Chat,
    id: &str,
    name: &str,
    admin: bool,
) -> awra::Widget {
    let row = awra::Box::new(awra::Orientation::Horizontal, 10);
    let face = awra::Avatar::new(32, Some(name));
    super::pictures::avatar(&face, app.avatar_path(id).as_deref());
    let label = awra::Label::new(Some(name));
    label.set_xalign(0.0);
    label.set_hexpand(true);
    label.set_ellipsize(awra::pango::EllipsizeMode::End);
    row.append(&face);
    row.append(&label);
    if admin {
        let badge = awra::Badge::new(gettext(app.locale, "Admin").as_ref());
        row.append(&badge);
    }
    let mine = app.our_ids().contains(&id);
    if mine {
        // Not a button: the same room as the buttons' rows around it.
        row.set_margin_start(8);
        row.set_margin_end(8);
        row.set_margin_top(4);
        row.set_margin_bottom(4);
        return row.upcast();
    }
    // A click opens their menu: their chat, and what an admin may do.
    let (weak, id, name, group) = (
        Rc::downgrade(driver),
        id.to_owned(),
        name.to_owned(),
        chat.id.clone(),
    );
    let anchor = row.clone();
    super::rows::button(&row, false, move || {
        let Some(driver) = weak.upgrade() else { return };
        let entries = driver.read(|app| {
            app.chat(&group)
                .map(|chat| super::group_admin::member_menu(&driver, app, chat, &id, &name))
                .unwrap_or_default()
        });
        awra::menu::popup_at(&anchor, 40.0, 20.0, entries);
    })
    .upcast()
}
