//! Dialogs that choose people or chats from a searchable list, read again
//! at every turn: New chat (a click starts the chat) and Forward (check the
//! chats, then forward, reviewing first when there are several).

use std::cell::Cell;
use std::rc::Rc;

use awra::prelude::*;

use super::Driver;
use crate::app::App;
use crate::i18n::{gettext, ngettext};
use crate::model::{Action, Dialog};

/// One row: a face, a name and a line under it, checked or not.
#[derive(Clone, Debug, PartialEq)]
struct Row {
    id: String,
    name: String,
    detail: String,
    picture: Option<std::path::PathBuf>,
    /// `None` for a list without check boxes.
    checked: Option<bool>,
    action: Action,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    NewChat,
    Forward,
}

pub struct Chooser {
    pub dialog: awra::Dialog,
    kind: Kind,
    search: awra::SearchEntry,
    list: awra::Box,
    empty: awra::Label,
    shown: Vec<Row>,
    /// Forward: the review of several recipients, and its list.
    review: awra::Label,
}

impl Chooser {
    /// The chooser for `dialog`, if it is one.
    pub fn of(
        driver: &Rc<Driver>,
        app: &App,
        window: &awra::Window,
        dialog: &Dialog,
    ) -> Option<Self> {
        let kind = match dialog {
            Dialog::NewChat if app.new_group.is_none() => Kind::NewChat,
            Dialog::Forward { .. } => Kind::Forward,
            _ => return None,
        };
        let locale = app.locale;
        let t = |text: &'static str| gettext(locale, text).into_owned();
        let shown = awra::Dialog::new(Some(window));
        shown.set_content_width(420);
        shown.set_default_size(440, 560);
        let content = awra::Box::new(awra::Orientation::Vertical, 8);
        let search = awra::SearchEntry::new();
        search.set_placeholder_text(Some(
            t(if kind == Kind::NewChat {
                "Search contacts"
            } else {
                "Search"
            })
            .as_str(),
        ));
        content.append(&search);
        if kind == Kind::NewChat {
            let shortcuts = awra::Box::new(awra::Orientation::Horizontal, 8);
            for (label, action) in [
                (t("New group"), Action::StartNewGroup),
                (t("Add contact"), Action::ShowDialog(Dialog::NewContact)),
            ] {
                let button = awra::Button::with_label(&label);
                let weak = Rc::downgrade(driver);
                button.connect_clicked(move |_| {
                    if let Some(driver) = weak.upgrade() {
                        driver.act(action.clone());
                    }
                });
                shortcuts.append(&button);
            }
            content.append(&shortcuts);
        }
        let review = awra::Label::new(None);
        review.set_wrap(true);
        review.set_xalign(0.0);
        review.set_visible(false);
        content.append(&review);
        let list = awra::Box::new(awra::Orientation::Vertical, 2);
        let scrolled = awra::ScrolledWindow::new();
        scrolled.set_hscrollbar_policy(awra::PolicyType::Never);
        scrolled.set_child(Some(&list));
        scrolled.set_vexpand(true);
        scrolled.set_min_content_height(320);
        content.append(&scrolled);
        let empty = awra::Label::new(Some(&t("No matching contacts")));
        empty.set_typography(awra::Typography::Muted);
        empty.set_visible(false);
        content.append(&empty);
        shown.set_child(Some(&content));

        match kind {
            Kind::NewChat => {
                shown.set_title(Some(t("New chat").as_str()));
                shown.add_response("cancel", &t("Close"));
                shown.set_close_response(Some("cancel"));
            }
            Kind::Forward => {
                shown.add_response("cancel", &t("Cancel"));
                shown.add_response("back", &t("Back"));
                shown.add_response("forward", &t("Forward"));
                shown.set_response_appearance("forward", awra::ButtonAppearance::Primary);
                shown.set_close_response(Some("cancel"));
                // The app closes the dialog once it forwards.
                shown.set_response_closes("forward", false);
                shown.set_response_closes("back", false);
            }
        }
        let answered = Rc::new(Cell::new(false));
        let weak = Rc::downgrade(driver);
        let replied = answered.clone();
        shown.connect_response(None, move |_, response| {
            let Some(driver) = weak.upgrade() else { return };
            match response {
                "forward" => {
                    let several = driver
                        .read(|app| !app.forward_reviewing && app.forward_recipients.len() > 1);
                    driver.act(if several {
                        Action::ReviewForward
                    } else {
                        Action::ConfirmForward
                    });
                }
                "back" => driver.act(Action::BackToForwardSelection),
                _ => {
                    replied.set(true);
                    driver.act(Action::CloseDialog);
                }
            }
        });
        super::dialogs::close_on_dismiss(driver, &shown, answered);

        let weak = Rc::downgrade(driver);
        search.connect_search_changed(move |entry| {
            let text = entry.text();
            if let Some(driver) = weak.upgrade() {
                driver.edit(move |app| match kind {
                    Kind::NewChat => app.new_chat_search = text,
                    Kind::Forward => app.forward_search = text,
                });
            }
        });
        Some(Self {
            dialog: shown,
            kind,
            search,
            list,
            empty,
            shown: Vec::new(),
            review,
        })
    }

    pub fn sync(&mut self, driver: &Rc<Driver>, app: &App) {
        let locale = app.locale;
        let rows = match self.kind {
            Kind::NewChat => new_chat_rows(app),
            Kind::Forward => forward_rows(app),
        };
        if self.kind == Kind::Forward {
            let count = match &app.dialog {
                Some(Dialog::Forward { messages, .. }) => messages.len(),
                _ => 1,
            };
            let heading = if count == 1 {
                gettext(locale, "Forward message").into_owned()
            } else {
                ngettext(
                    locale,
                    "Forward {count} message",
                    "Forward {count} messages",
                    count as u32,
                )
                .replace("{count}", &count.to_string())
            };
            self.dialog.set_title(Some(heading.as_str()));
            let reviewing = app.forward_reviewing;
            self.search.set_visible(!reviewing);
            self.review.set_visible(reviewing);
            if reviewing {
                let names: Vec<String> = app
                    .forward_recipients
                    .iter()
                    .filter_map(|id| app.chat(id).map(|chat| app.chat_title(chat)))
                    .collect();
                self.review.set_text(&format!(
                    "{}\n\n{}",
                    gettext(locale, "Forward to these chats?"),
                    names.join("\n")
                ));
            }
            let chosen = app.forward_recipients.len();
            self.dialog.set_response_enabled("forward", chosen > 0);
            self.dialog.set_response_enabled("back", reviewing);
            self.dialog.set_response_label(
                "forward",
                gettext(
                    locale,
                    if !reviewing && chosen > 1 {
                        "Review recipients"
                    } else {
                        "Forward"
                    },
                )
                .as_ref(),
            );
        }
        let search = match self.kind {
            Kind::NewChat => &app.new_chat_search,
            Kind::Forward => &app.forward_search,
        };
        if self.search.text() != *search {
            self.search.set_text(search);
        }
        let reviewing = self.kind == Kind::Forward && app.forward_reviewing;
        self.empty.set_visible(rows.is_empty() && !reviewing);
        self.list.set_visible(!reviewing);
        if rows == self.shown {
            return;
        }
        while let Some(child) = self.list.first_child() {
            self.list.remove(&child);
        }
        for row in &rows {
            self.list.append(&row_widget(driver, row));
        }
        self.shown = rows;
    }
}

fn new_chat_rows(app: &App) -> Vec<Row> {
    let needle = crate::util::search_key(app.new_chat_search.trim());
    let mut rows = Vec::new();
    if app.offers_self(&needle) {
        rows.push(Row {
            id: String::from("self"),
            name: app.self_title(),
            detail: gettext(app.locale, "Message yourself").into_owned(),
            picture: app.me.as_deref().and_then(|me| app.avatar_path(me)),
            checked: None,
            action: Action::MessageYourself,
        });
    }
    for contact in crate::ui::dialogs::contact_choices(app, &needle) {
        let name = app.display_name(&contact.id);
        rows.push(Row {
            detail: crate::model::phone_of(&contact.id)
                .map(crate::util::phone)
                .unwrap_or_default(),
            picture: app.avatar_path(&contact.id),
            checked: None,
            action: Action::StartChat {
                id: contact.id.clone(),
                name: name.clone(),
                dismiss_dialog: true,
            },
            id: contact.id,
            name,
        });
    }
    rows
}

fn forward_rows(app: &App) -> Vec<Row> {
    let needle = app.forward_search.trim().to_lowercase();
    app.chats
        .iter()
        .filter(|chat| crate::ui::dialogs::forwardable(chat) && !app.blocklist.contains(&chat.id))
        .filter(|chat| {
            needle.is_empty()
                || app.chat_title(chat).to_lowercase().contains(&needle)
                || chat.phone().is_some_and(|phone| phone.contains(&needle))
        })
        .take(200)
        .map(|chat| Row {
            id: chat.id.clone(),
            name: app.chat_title(chat),
            detail: if chat.is_group() {
                gettext(app.locale, "Group").into_owned()
            } else {
                chat.phone().map(crate::util::phone).unwrap_or_default()
            },
            picture: app.avatar_path(&chat.id),
            checked: Some(app.forward_recipients.contains(&chat.id)),
            action: Action::ToggleForwardRecipient(chat.id.clone()),
        })
        .collect()
}

fn row_widget(driver: &Rc<Driver>, row: &Row) -> awra::Box {
    let line = awra::Box::new(awra::Orientation::Horizontal, 12);
    line.set_margin_top(4);
    line.set_margin_bottom(4);
    line.set_margin_start(4);
    line.set_margin_end(4);
    let face = awra::Avatar::new(36, Some(&row.name));
    let picture = row.picture.as_deref().and_then(super::pictures::file);
    face.set_custom_image(picture.as_ref());
    line.append(&face);
    let lines = awra::Box::new(awra::Orientation::Vertical, 0);
    lines.set_hexpand(true);
    lines.set_valign(awra::Align::Center);
    let name = awra::Label::new(Some(&row.name));
    name.set_xalign(0.0);
    name.set_weight(600);
    name.set_ellipsize(awra::pango::EllipsizeMode::End);
    lines.append(&name);
    if !row.detail.is_empty() {
        let detail = awra::Label::new(Some(&row.detail));
        detail.set_xalign(0.0);
        detail.set_typography(awra::Typography::Muted);
        lines.append(&detail);
    }
    line.append(&lines);
    if let Some(checked) = row.checked {
        let check = awra::CheckButton::new();
        check.set_active(checked);
        check.set_can_target(false);
        check.set_valign(awra::Align::Center);
        line.append(&check);
    }
    let click = awra::GestureClick::new();
    let (weak, action) = (Rc::downgrade(driver), row.action.clone());
    click.connect_released(move |_, _, _, _| {
        if let Some(driver) = weak.upgrade() {
            driver.act(action.clone());
        }
    });
    line.add_controller(click);
    line
}
