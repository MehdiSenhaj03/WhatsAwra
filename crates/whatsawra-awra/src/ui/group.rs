//! New group and Add members: the app's `new_group` draft drawn as the
//! egui dialog draws it. Choosing has check boxes and the picked people as
//! chips; naming (a new group only) asks for the name and a photo.

use std::cell::Cell;
use std::rc::{Rc, Weak};

use awra::prelude::*;

use super::Driver;
use super::chooser::{Row, row_widget};
use super::dialogs::Live;
use whatsawra::app::App;
use whatsawra::i18n::{gettext, ngettext, pgettext};
use whatsawra::model::{Action, Dialog};
use whatsawra::new_group::Stage;

/// The dialog for `dialog` while a group is being put together.
pub fn dialog_of(
    driver: &Rc<Driver>,
    app: &App,
    window: &awra::Window,
    dialog: &Dialog,
) -> Option<Box<dyn Live>> {
    match dialog {
        Dialog::NewChat | Dialog::AddGroupMembers(_) if app.new_group.is_some() => {
            Some(Box::new(NewGroup::new(driver, app, window)))
        }
        _ => None,
    }
}

struct NewGroup {
    dialog: awra::Dialog,
    pages: awra::Stack,
    chips: awra::FlowBox,
    search: awra::SearchEntry,
    list: awra::Box,
    empty: awra::Label,
    count: awra::Label,
    photo: awra::Avatar,
    add_photo: awra::Button,
    remove_photo: awra::Button,
    name: awra::Entry,
    members: awra::Label,
    problem: awra::Label,
    shown_rows: Vec<Row>,
    shown_chips: Vec<(String, String)>,
    shown_photo: Option<std::path::PathBuf>,
    driver: Weak<Driver>,
}

fn muted() -> awra::Label {
    let label = awra::Label::new(None);
    label.set_xalign(0.0);
    label.set_wrap(true);
    label.set_typography(awra::Typography::Muted);
    label
}

impl NewGroup {
    fn new(driver: &Rc<Driver>, app: &App, window: &awra::Window) -> Self {
        let t = |text: &'static str| gettext(app.locale, text).into_owned();
        let act = |action: Action| {
            let weak = Rc::downgrade(driver);
            move |_: &awra::Button| {
                if let Some(driver) = weak.upgrade() {
                    driver.act(action.clone());
                }
            }
        };
        let dialog = awra::Dialog::new(Some(window));
        dialog.set_content_width(420);
        dialog.set_default_size(440, 600);
        let pages = awra::Stack::new();
        pages.set_margin_start(16);
        pages.set_margin_end(16);
        pages.set_margin_bottom(8);

        // Choosing: chips of the picked, a search, contacts with check boxes.
        let choosing = awra::Box::new(awra::Orientation::Vertical, 8);
        let chips = awra::FlowBox::new();
        chips.set_row_spacing(4);
        chips.set_column_spacing(4);
        choosing.append(&chips);
        let search = awra::SearchEntry::new();
        search.set_placeholder_text(t("Search contacts"));
        let weak = Rc::downgrade(driver);
        search.connect_search_changed(move |entry| {
            let text = entry.text();
            if let Some(driver) = weak.upgrade() {
                driver.edit(move |app| app.new_chat_search = text);
            }
        });
        choosing.append(&search);
        let list = awra::Box::new(awra::Orientation::Vertical, 2);
        let scrolled = awra::ScrolledWindow::new();
        scrolled.set_hscrollbar_policy(awra::PolicyType::Never);
        scrolled.set_child(Some(&list));
        scrolled.set_vexpand(true);
        scrolled.set_min_content_height(300);
        choosing.append(&scrolled);
        let empty = muted();
        empty.set_text(&t("No matching contacts"));
        choosing.append(&empty);
        let count = muted();
        choosing.append(&count);
        pages.add_named(&choosing, Some("choosing"));

        // Naming: the photo, the name, who is in it.
        let naming = awra::Box::new(awra::Orientation::Vertical, 10);
        let photo = awra::Avatar::new(96, None);
        photo.set_halign(awra::Align::Center);
        naming.append(&photo);
        let photo_buttons = awra::Box::new(awra::Orientation::Horizontal, 8);
        photo_buttons.set_halign(awra::Align::Center);
        let add_photo = awra::Button::with_label(&t("Add group photo"));
        add_photo.connect_clicked(act(Action::PickNewGroupPicture));
        let remove_photo = awra::Button::with_label(&t("Remove photo"));
        remove_photo.set_appearance(awra::ButtonAppearance::Ghost);
        remove_photo.connect_clicked(act(Action::RemoveNewGroupPicture));
        photo_buttons.append(&add_photo);
        photo_buttons.append(&remove_photo);
        naming.append(&photo_buttons);
        let name = awra::Entry::new();
        name.set_placeholder_text(t("Group name (required)"));
        let weak = Rc::downgrade(driver);
        name.connect_changed(move |entry| {
            let text = entry.text();
            if let Some(driver) = weak.upgrade() {
                driver.edit(move |app| {
                    if let Some(draft) = app.new_group.as_mut() {
                        draft.name = text;
                    }
                });
            }
        });
        naming.append(&name);
        let members = muted();
        naming.append(&members);
        let problem = muted();
        naming.append(&problem);
        pages.add_named(&naming, Some("naming"));
        dialog.set_child(Some(&pages));

        dialog.add_response("cancel", &t("Close"));
        dialog.add_response("back", &t("Back"));
        dialog.add_response("next", &t("Next"));
        dialog.set_response_appearance("next", awra::ButtonAppearance::Primary);
        dialog.set_response_closes("next", false);
        dialog.set_response_closes("back", false);
        dialog.set_close_response(Some("cancel"));
        let answered = Rc::new(Cell::new(false));
        let (weak, replied) = (Rc::downgrade(driver), answered.clone());
        dialog.connect_response(None, move |_, response| {
            let Some(driver) = weak.upgrade() else { return };
            match response {
                "next" => {
                    let next = driver.read(|app| {
                        let draft = app.new_group.as_ref()?;
                        Some(if draft.adding.is_some() {
                            Action::AddGroupMembers
                        } else if draft.stage == Stage::Choosing {
                            Action::NewGroupNext
                        } else {
                            Action::CreateGroup
                        })
                    });
                    if let Some(next) = next {
                        driver.act(next);
                    }
                }
                "back" => driver.act(Action::NewGroupBack),
                _ => {
                    replied.set(true);
                    driver.act(Action::CloseDialog);
                }
            }
        });
        super::dialogs::close_on_dismiss(driver, &dialog, answered);
        Self {
            dialog,
            pages,
            chips,
            search,
            list,
            empty,
            count,
            photo,
            add_photo,
            remove_photo,
            name,
            members,
            problem,
            shown_rows: Vec::new(),
            shown_chips: Vec::new(),
            shown_photo: None,
            driver: Rc::downgrade(driver),
        }
    }

    fn sync_choosing(&mut self, driver: &Rc<Driver>, app: &App) {
        let Some(draft) = app.new_group.as_ref() else {
            return;
        };
        if self.search.text() != app.new_chat_search {
            self.search.set_text(&app.new_chat_search);
        }
        let chips: Vec<(String, String)> = draft
            .members
            .iter()
            .map(|id| (id.clone(), app.display_name(id)))
            .collect();
        if chips != self.shown_chips {
            while let Some(child) = self.chips.first_child() {
                self.chips.remove(&child);
            }
            for (id, name) in &chips {
                let chip = awra::Chip::new(Some(name));
                chip.set_icon_name(Some("window-close-symbolic"));
                let (weak, id) = (self.driver.clone(), id.clone());
                chip.connect_activate(move |_| {
                    if let Some(driver) = weak.upgrade() {
                        driver.act(Action::ToggleNewGroupMember(id.clone()));
                    }
                });
                self.chips.append(&chip);
            }
            self.chips.set_visible(!chips.is_empty());
            self.shown_chips = chips;
        }
        let needle = whatsawra::util::search_key(app.new_chat_search.trim());
        let rows: Vec<Row> = whatsawra::ui::dialogs::contact_choices(app, &needle)
            .into_iter()
            .filter(|contact| draft.offers(&contact.id))
            .map(|contact| Row {
                name: app.display_name(&contact.id),
                detail: whatsawra::model::phone_of(&contact.id)
                    .map(whatsawra::util::phone)
                    .unwrap_or_default(),
                picture: app.avatar_path(&contact.id),
                checked: Some(draft.picked(&contact.id)),
                action: Action::ToggleNewGroupMember(contact.id.clone()),
                id: contact.id,
            })
            .collect();
        self.empty.set_visible(rows.is_empty());
        if rows != self.shown_rows {
            while let Some(child) = self.list.first_child() {
                self.list.remove(&child);
            }
            for row in &rows {
                self.list.append(&row_widget(driver, row));
            }
            self.shown_rows = rows;
        }
        let count = draft.members.len();
        self.count.set_text(&if draft.creating {
            gettext(app.locale, "Creating the group…").into_owned()
        } else {
            ngettext(
                app.locale,
                "{count} selected",
                "{count} selected",
                count as u32,
            )
            .replace("{count}", &count.to_string())
        });
    }

    fn sync_naming(&mut self, app: &App) {
        let Some(draft) = app.new_group.as_ref() else {
            return;
        };
        let locale = app.locale;
        if self.name.text() != draft.name {
            self.name.set_text(&draft.name);
        }
        self.photo.set_text(Some(if draft.name.trim().is_empty() {
            "?"
        } else {
            draft.name.trim()
        }));
        if self.shown_photo != draft.picture {
            super::pictures::avatar(&self.photo, draft.picture.as_deref());
            self.shown_photo = draft.picture.clone();
        }
        self.add_photo.set_label(
            gettext(
                locale,
                if draft.picture.is_some() {
                    "Change photo"
                } else {
                    "Add group photo"
                },
            )
            .as_ref(),
        );
        self.remove_photo.set_visible(draft.picture.is_some());
        let names: Vec<String> = draft
            .members
            .iter()
            .map(|id| app.display_name(id))
            .collect();
        self.members.set_text(
            &ngettext(
                locale,
                "{count} member: {names}",
                "{count} members: {names}",
                names.len() as u32,
            )
            .replace("{count}", &names.len().to_string())
            .replace("{names}", &names.join(", ")),
        );
        let problem = if draft.creating {
            gettext(locale, "Creating the group…").into_owned()
        } else {
            draft
                .problem()
                .map(|problem| problem.message(locale))
                .unwrap_or_default()
        };
        self.problem.set_visible(!problem.is_empty());
        self.problem.set_text(&problem);
    }
}

impl Live for NewGroup {
    fn dialog(&self) -> awra::Dialog {
        self.dialog.clone()
    }

    fn sync(&mut self, driver: &Rc<Driver>, app: &App) {
        let Some(draft) = app.new_group.as_ref() else {
            return;
        };
        let locale = app.locale;
        let adding = draft.adding.is_some();
        let naming = draft.stage == Stage::Naming && !adding;
        let page = if naming { "naming" } else { "choosing" };
        if self.pages.visible_child_name().as_deref() != Some(page) {
            self.pages.set_visible_child_name(page);
        }
        let title = if naming {
            gettext(locale, "New group")
        } else if adding {
            gettext(locale, "Add members")
        } else {
            gettext(locale, "Add group members")
        };
        self.dialog.set_title(Some(title.as_ref()));
        let (next, ready) = if adding {
            (
                pgettext(locale, "add group members", "Add").into_owned(),
                draft.can_add(),
            )
        } else if naming {
            (gettext(locale, "Create").into_owned(), draft.can_create())
        } else {
            (gettext(locale, "Next").into_owned(), draft.can_advance())
        };
        self.dialog.set_response_label("next", &next);
        self.dialog
            .set_response_enabled("next", ready && !draft.creating);
        // Back leaves the group for the contact list; Add members has none.
        self.dialog
            .set_response_enabled("back", !adding && !draft.creating);
        if naming {
            self.sync_naming(app);
        } else {
            self.sync_choosing(driver, app);
        }
    }
}
