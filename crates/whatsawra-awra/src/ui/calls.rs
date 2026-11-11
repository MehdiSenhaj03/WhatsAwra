//! The Calls page's list: every call of the account, newest first, a row
//! per run of alike calls (the egui page's rows and words), each with the
//! button that calls back. A picked row opens its chat beside the list.

use std::rc::Rc;

use awra::prelude::*;

use super::Driver;
use whatsawra::app::App;
use whatsawra::i18n::{Locale, gettext};
use whatsawra::model::{Action, CallFilter};
use whatsawra::ui::calls::{Entry, row_name, rows, summary, visible_calls};

/// What one row shows.
#[derive(Clone, Debug, PartialEq)]
struct Row {
    chat: String,
    message: String,
    name: String,
    count: usize,
    summary: String,
    stamp: String,
    missed: bool,
    video: bool,
    outgoing: bool,
    /// Whether this call can be returned from here.
    callable: bool,
    picked: bool,
    picture: Option<std::path::PathBuf>,
}

pub struct CallList {
    pub root: awra::Box,
    chips: Vec<(CallFilter, awra::Chip)>,
    list: awra::Box,
    empty: awra::EmptyState,
    shown: Vec<Row>,
}

impl CallList {
    pub fn build(driver: &Rc<Driver>, locale: Locale) -> Self {
        let root = awra::Box::new(awra::Orientation::Vertical, 8);
        root.set_margin_top(8);

        let chip_row = awra::Box::new(awra::Orientation::Horizontal, 6);
        chip_row.set_margin_start(12);
        let mut chips = Vec::new();
        for filter in CallFilter::EVERY {
            let chip = awra::Chip::new(Some(filter.label(locale).as_ref()));
            chip.set_toggleable(false);
            let weak = Rc::downgrade(driver);
            chip.connect_activate(move |_| {
                if let Some(driver) = weak.upgrade() {
                    driver.act(Action::SetCallFilter(filter));
                }
            });
            chip_row.append(&chip);
            chips.push((filter, chip));
        }
        root.append(&chip_row);

        let list = awra::Box::new(awra::Orientation::Vertical, 2);
        list.set_margin_start(8);
        list.set_margin_end(8);
        let scrolled = awra::ScrolledWindow::new();
        scrolled.set_hscrollbar_policy(awra::PolicyType::Never);
        scrolled.set_child(Some(&list));
        scrolled.set_vexpand(true);
        root.append(&scrolled);

        let empty = awra::EmptyState::new();
        empty.set_icon_name(Some("awra-call-symbolic"));
        empty.set_title(Some(gettext(locale, "No calls yet").as_ref()));
        empty.set_vexpand(true);
        empty.set_visible(false);
        root.append(&empty);

        Self {
            root,
            chips,
            list,
            empty,
            shown: Vec::new(),
        }
    }

    pub fn sync(&mut self, driver: &Rc<Driver>, app: &App) {
        for (filter, chip) in &self.chips {
            chip.set_active(app.call_filter == *filter);
        }
        let calls = visible_calls(app);
        let locale = app.locale;
        let rows: Vec<Row> = rows(&calls)
            .into_iter()
            .filter_map(|row| {
                let newest = *row.first()?;
                let entry = Entry::of(newest)?;
                if app.call_filter == CallFilter::Missed && !entry.missed() {
                    return None;
                }
                let link = newest.chat == whatsawra::model::CALL_LINK_CHAT;
                let group_chat = whatsawra::model::ChatKind::from_id(&newest.chat)
                    == whatsawra::model::ChatKind::Group
                    && whatsawra::calls::group_calls_enabled();
                Some(Row {
                    chat: newest.chat.clone(),
                    message: newest.id.clone(),
                    name: row_name(app, &newest.chat),
                    count: row.len(),
                    summary: summary(&entry, locale),
                    stamp: whatsawra::util::chat_stamp(locale, newest.timestamp),
                    missed: entry.missed(),
                    video: entry.video,
                    outgoing: entry.from_me,
                    callable: !link && (!entry.group || group_chat),
                    picked: app
                        .picked_call
                        .as_ref()
                        .is_some_and(|(chat, id)| *chat == newest.chat && *id == newest.id),
                    picture: app.avatar_path(&newest.chat),
                })
            })
            .collect();
        self.empty.set_visible(rows.is_empty());
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

fn row_widget(driver: &Rc<Driver>, row: &Row) -> awra::Widget {
    let line = awra::Box::new(awra::Orientation::Horizontal, 12);
    let avatar = awra::Avatar::new(40, Some(&row.name));
    super::pictures::avatar(&avatar, row.picture.as_deref());
    line.append(&avatar);

    let lines = awra::Box::new(awra::Orientation::Vertical, 1);
    lines.set_hexpand(true);
    lines.set_valign(awra::Align::Center);
    let name = awra::Label::new(None);
    name.set_xalign(0.0);
    name.set_ellipsize(awra::pango::EllipsizeMode::End);
    let title = if row.count > 1 {
        format!("{} ({})", row.name, row.count)
    } else {
        row.name.clone()
    };
    if row.missed {
        name.set_markup(&format!(
            "<span foreground=\"#e5484d\">{}</span>",
            awra::glib::markup_escape_text(&title)
        ));
    } else {
        name.set_text(&title);
    }
    name.set_weight(600);
    let arrow = if row.missed {
        "↙"
    } else if row.outgoing {
        "↗"
    } else {
        "↙"
    };
    let detail = awra::Label::new(Some(&format!("{arrow} {} · {}", row.summary, row.stamp)));
    detail.set_xalign(0.0);
    detail.set_typography(awra::Typography::Muted);
    detail.set_ellipsize(awra::pango::EllipsizeMode::End);
    lines.append(&name);
    lines.append(&detail);
    line.append(&lines);

    if row.callable {
        let back = awra::Button::from_icon_name(if row.video {
            "awra-video-call-symbolic"
        } else {
            "awra-call-symbolic"
        });
        back.set_appearance(awra::ButtonAppearance::Ghost);
        back.set_valign(awra::Align::Center);
        let (weak, chat, video) = (Rc::downgrade(driver), row.chat.clone(), row.video);
        back.connect_clicked(move |_| {
            if let Some(driver) = weak.upgrade() {
                driver.act(if video {
                    Action::StartVideoCall(chat.clone())
                } else {
                    Action::StartCall(chat.clone())
                });
            }
        });
        line.append(&back);
    }
    // A call by link belongs to no chat: its row opens nothing.
    if row.chat == whatsawra::model::CALL_LINK_CHAT {
        return line.upcast();
    }
    let (weak, chat, message) = (Rc::downgrade(driver), row.chat.clone(), row.message.clone());
    super::rows::button(&line, row.picked, move || {
        if let Some(driver) = weak.upgrade() {
            driver.act(Action::PickCall {
                chat: chat.clone(),
                message: message.clone(),
            });
        }
    })
    .upcast()
}
