//! Running a group from its info panel, as the egui panel does: its name,
//! description and photo for those who may edit them, Add members, the
//! invite link, the permissions an admin sets, and each member's menu.
//! Each change goes out as the app's action; the worker files it once
//! WhatsApp accepts it.

use std::rc::Rc;

use awra::prelude::*;

use super::Driver;
use whatsawra::app::App;
use whatsawra::group_admin::{LinkState, MemberChange, Permission};
use whatsawra::i18n::{Locale, gettext};
use whatsawra::model::{Action, Chat, Dialog};

/// The chat the info panel shows.
fn info_chat(driver: &Rc<Driver>) -> Option<String> {
    driver.read(|app| app.info.as_ref().map(|info| info.chat.clone()))
}

fn row_button(label: &str, icon: &str) -> awra::Button {
    let button = awra::Button::new();
    button.set_appearance(awra::ButtonAppearance::Ghost);
    let line = awra::Box::new(awra::Orientation::Horizontal, 10);
    line.append(&awra::Image::from_icon_name(icon));
    let text = awra::Label::new(Some(label));
    text.set_xalign(0.0);
    text.set_hexpand(true);
    line.append(&text);
    button.set_child(Some(&line));
    button
}

/// A small dialog with one field, for the group's name or description.
fn edit_text(
    driver: &Rc<Driver>,
    locale: Locale,
    title: &str,
    current: &str,
    multiline: bool,
    done: impl Fn(String) -> Action + 'static,
) {
    let window = driver
        .shell
        .borrow()
        .as_ref()
        .map(|shell| shell.window.clone());
    let dialog = awra::Dialog::new(window.as_ref());
    dialog.set_title(Some(title));
    dialog.set_content_width(420);
    // A description takes several lines; a name one.
    let view = awra::TextView::new();
    view.set_text(current);
    view.set_wrap_mode(awra::WrapMode::WordChar);
    view.set_size_request(-1, 120);
    let entry = awra::Entry::new();
    entry.set_text(current);
    let field: awra::Widget = if multiline {
        view.clone().upcast()
    } else {
        entry.clone().upcast()
    };
    field.set_margin_start(16);
    field.set_margin_end(16);
    field.set_margin_bottom(8);
    dialog.set_child(Some(&field));
    dialog.add_response("cancel", gettext(locale, "Cancel").as_ref());
    dialog.add_response("save", gettext(locale, "Save").as_ref());
    dialog.set_response_appearance("save", awra::ButtonAppearance::Primary);
    dialog.set_close_response(Some("cancel"));
    let weak = Rc::downgrade(driver);
    dialog.connect_response(Some("save"), move |_, _| {
        let text = if multiline { view.text() } else { entry.text() };
        if let Some(driver) = weak.upgrade() {
            driver.act(done(text));
        }
    });
    dialog.present();
}

pub struct GroupAdmin {
    pub root: awra::Box,
    rename: awra::Button,
    describe: awra::Button,
    photo: awra::Button,
    add: awra::Button,
    invite: awra::Section,
    invite_text: awra::Label,
    copy: awra::Button,
    reset: awra::Button,
    permissions: awra::Section,
    switches: Vec<(Permission, awra::Switch)>,
    locale: Locale,
    /// The group whose link was asked for, so it is asked once.
    asked: Option<String>,
}

impl GroupAdmin {
    pub fn build(driver: &Rc<Driver>, locale: Locale) -> Self {
        let t = |text: &'static str| gettext(locale, text).into_owned();
        let root = awra::Box::new(awra::Orientation::Vertical, 12);

        let manage = awra::Box::new(awra::Orientation::Vertical, 2);
        let rename = row_button(&t("Edit group name"), "document-edit-symbolic");
        let weak = Rc::downgrade(driver);
        rename.connect_clicked(move |_| {
            let Some(driver) = weak.upgrade() else { return };
            let Some(chat) = info_chat(&driver) else {
                return;
            };
            let (current, locale) = driver.read(|app| {
                (
                    app.chat(&chat)
                        .map(|c| app.chat_title(c))
                        .unwrap_or_default(),
                    app.locale,
                )
            });
            let title = gettext(locale, "Group name").into_owned();
            edit_text(&driver, locale, &title, &current, false, move |name| {
                Action::SetGroupName {
                    chat: chat.clone(),
                    name,
                }
            });
        });
        let describe = row_button(&t("Edit description"), "x-office-document-symbolic");
        let weak = Rc::downgrade(driver);
        describe.connect_clicked(move |_| {
            let Some(driver) = weak.upgrade() else { return };
            let Some(chat) = info_chat(&driver) else {
                return;
            };
            let (current, locale) = driver.read(|app| {
                (
                    app.chat(&chat)
                        .and_then(|c| c.group_description.clone())
                        .unwrap_or_default(),
                    app.locale,
                )
            });
            let title = gettext(locale, "Group description").into_owned();
            edit_text(
                &driver,
                locale,
                &title,
                &current,
                true,
                move |description| Action::SetGroupDescription {
                    chat: chat.clone(),
                    description,
                },
            );
        });
        let photo = row_button(&t("Change group photo"), "camera-photo-symbolic");
        let weak = Rc::downgrade(driver);
        photo.connect_clicked(move |button| {
            let Some(driver) = weak.upgrade() else { return };
            let Some(chat) = info_chat(&driver) else {
                return;
            };
            let has_photo = driver.read(|app| app.avatar_path(&chat).is_some());
            let on = |action: Action| {
                let weak = Rc::downgrade(&driver);
                move || {
                    if let Some(driver) = weak.upgrade() {
                        driver.act(action.clone());
                    }
                }
            };
            let mut entries = vec![
                awra::menu::Entry::item(
                    gettext(locale, "Choose a photo").into_owned(),
                    on(Action::PickGroupPicture(chat.clone())),
                )
                .icon("document-open-symbolic"),
            ];
            if has_photo {
                entries.push(
                    awra::menu::Entry::item(
                        gettext(locale, "Remove photo").into_owned(),
                        on(Action::RemoveGroupPicture(chat)),
                    )
                    .icon("user-trash-symbolic")
                    .destructive(),
                );
            }
            awra::menu::popup_at(button, 0.0, 0.0, entries);
        });
        let add = row_button(&t("Add members"), "list-add-symbolic");
        let weak = Rc::downgrade(driver);
        add.connect_clicked(move |_| {
            let Some(driver) = weak.upgrade() else { return };
            if let Some(chat) = info_chat(&driver) {
                driver.act(Action::ShowDialog(Dialog::AddGroupMembers(chat)));
            }
        });
        manage.append(&rename);
        manage.append(&describe);
        manage.append(&photo);
        manage.append(&add);
        root.append(&manage);

        let invite = awra::Section::new();
        invite.set_title(Some(t("Invite via link").as_str()));
        let invite_text = awra::Label::new(None);
        invite_text.set_xalign(0.0);
        invite_text.set_wrap(true);
        invite_text.set_wrap_mode(awra::WrapMode::WordChar);
        invite_text.set_selectable(true);
        invite.append(&invite_text);
        let invite_buttons = awra::Box::new(awra::Orientation::Horizontal, 8);
        let copy = awra::Button::with_label(&t("Copy link"));
        let weak = Rc::downgrade(driver);
        copy.connect_clicked(move |_| {
            let Some(driver) = weak.upgrade() else { return };
            if let Some(chat) = info_chat(&driver) {
                let failed = driver.read(|app| {
                    matches!(
                        app.info_invite.as_ref(),
                        Some((shown, LinkState::Failed(_))) if *shown == chat
                    )
                });
                driver.act(if failed {
                    Action::FetchInviteLink(chat)
                } else {
                    Action::CopyInviteLink(chat)
                });
            }
        });
        let reset = awra::Button::with_label(&t("Reset link"));
        reset.set_appearance(awra::ButtonAppearance::Ghost);
        awra::add_style_role(&reset, "quiet-destructive");
        let weak = Rc::downgrade(driver);
        reset.connect_clicked(move |_| {
            let Some(driver) = weak.upgrade() else { return };
            if let Some(chat) = info_chat(&driver) {
                driver.act(Action::ShowDialog(Dialog::ConfirmResetInviteLink(chat)));
            }
        });
        invite_buttons.append(&copy);
        invite_buttons.append(&reset);
        invite.append(&invite_buttons);
        root.append(&invite);

        let permissions = awra::Section::new();
        permissions.set_title(Some(t("Group permissions").as_str()));
        let mut switches = Vec::new();
        for permission in Permission::ALL {
            let line = awra::Box::new(awra::Orientation::Horizontal, 10);
            let lines = awra::Box::new(awra::Orientation::Vertical, 0);
            lines.set_hexpand(true);
            let label = awra::Label::new(Some(&permission.label(locale)));
            label.set_xalign(0.0);
            label.set_wrap(true);
            let about = awra::Label::new(Some(&permission.description(locale)));
            about.set_xalign(0.0);
            about.set_wrap(true);
            about.set_typography(awra::Typography::Muted);
            lines.append(&label);
            lines.append(&about);
            let switch = awra::Switch::new();
            switch.set_valign(awra::Align::Center);
            let weak = Rc::downgrade(driver);
            switch.connect_active_notify(move |switch| {
                let on = switch.is_active();
                let Some(driver) = weak.upgrade() else { return };
                let Some(chat) = info_chat(&driver) else {
                    return;
                };
                // Only the reader's own flips: the view sets it too.
                let current = driver.read(|app| app.chat(&chat).and_then(|c| permission.value(c)));
                if current.is_some_and(|value| value != on) {
                    driver.act(Action::SetGroupPermission {
                        chat,
                        permission,
                        members: on,
                    });
                }
            });
            line.append(&lines);
            line.append(&switch);
            permissions.append(&line);
            switches.push((permission, switch));
        }
        root.append(&permissions);

        Self {
            root,
            rename,
            describe,
            photo,
            add,
            invite,
            invite_text,
            copy,
            reset,
            permissions,
            switches,
            locale,
            asked: None,
        }
    }

    /// Shows what the reader may do to `chat`; hidden for other chats.
    pub fn sync(&mut self, driver: &Rc<Driver>, app: &App, chat: &Chat) {
        if !chat.is_group() || chat.left {
            self.root.set_visible(false);
            self.asked = None;
            return;
        }
        self.root.set_visible(true);
        let edit = chat.can_edit_info();
        self.rename.set_visible(edit);
        self.describe.set_visible(edit);
        self.photo.set_visible(edit);
        self.add.set_visible(chat.can_add_members());

        let sharing = chat.can_share_invite_link();
        self.invite.set_visible(sharing);
        if sharing {
            // The link is asked for when the panel shows it, as egui does.
            let known = app
                .info_invite
                .as_ref()
                .filter(|(shown, _)| *shown == chat.id)
                .map(|(_, state)| state);
            if known.is_none() && self.asked.as_deref() != Some(chat.id.as_str()) {
                self.asked = Some(chat.id.clone());
                driver.later(super::Op::Action(Action::FetchInviteLink(chat.id.clone())));
            }
            let t = |text: &'static str| gettext(self.locale, text).into_owned();
            let (text, link) = match known {
                Some(LinkState::Known(link)) => (link.as_str().to_owned(), true),
                Some(LinkState::Failed(failure)) => (failure.link_message(self.locale), false),
                _ => (t("Getting the link…"), false),
            };
            self.invite_text.set_text(&text);
            let failed = matches!(known, Some(LinkState::Failed(_)));
            self.copy.set_label(
                if failed {
                    t("Try again")
                } else {
                    t("Copy link")
                }
                .as_str(),
            );
            self.copy.set_sensitive(link || failed);
            self.reset.set_visible(chat.can_manage_group());
            self.reset.set_sensitive(link);
        } else {
            self.asked = None;
        }

        let manage = chat.can_manage_group();
        self.permissions.set_visible(manage);
        if manage {
            for (permission, switch) in &self.switches {
                let value = permission.value(chat);
                switch.set_sensitive(value.is_some());
                if let Some(value) = value
                    && switch.is_active() != value
                {
                    switch.set_active(value);
                }
            }
        }
    }
}

/// A member's menu: their chat, and what an admin may make of them.
pub fn member_menu(
    driver: &Rc<Driver>,
    app: &App,
    chat: &Chat,
    member: &str,
    name: &str,
) -> Vec<awra::menu::Entry> {
    let locale = app.locale;
    let on = |action: Action| {
        let weak = Rc::downgrade(driver);
        move || {
            if let Some(driver) = weak.upgrade() {
                driver.act(action.clone());
            }
        }
    };
    let message = gettext(locale, "Message {name}").replace("{name}", name);
    let mut entries = vec![
        awra::menu::Entry::item(message, {
            let (weak, id, name) = (Rc::downgrade(driver), member.to_owned(), name.to_owned());
            move || {
                if let Some(driver) = weak.upgrade() {
                    driver.act(Action::StartChat {
                        id: id.clone(),
                        name: name.clone(),
                        dismiss_dialog: false,
                    });
                    driver.act(Action::CloseInfo);
                }
            }
        })
        .icon("awra-chats-symbolic"),
    ];
    if chat.can_manage_group() {
        let change = |change: MemberChange| Action::ChangeGroupMember {
            chat: chat.id.clone(),
            member: member.to_owned(),
            change,
        };
        entries.push(awra::menu::Entry::separator());
        entries.push(if chat.is_group_admin(member) {
            awra::menu::Entry::item(
                gettext(locale, "Dismiss as admin").into_owned(),
                on(change(MemberChange::Demote)),
            )
        } else {
            awra::menu::Entry::item(
                gettext(locale, "Make group admin").into_owned(),
                on(change(MemberChange::Promote)),
            )
        });
        entries.push(
            awra::menu::Entry::item(
                gettext(locale, "Remove {name}").replace("{name}", name),
                on(Action::ShowDialog(Dialog::ConfirmRemoveMember {
                    chat: chat.id.clone(),
                    member: member.to_owned(),
                })),
            )
            .icon("user-trash-symbolic")
            .destructive(),
        );
    }
    entries
}
