//! Search inside the open chat: a pane beside it (the info panel's place),
//! with the app's answer to the query, newest first. A hit opens its
//! message, which the conversation brings to the middle and flashes.

use std::rc::Rc;

use awra::prelude::*;

use super::Driver;
use whatsawra::app::App;
use whatsawra::i18n::{Locale, gettext};
use whatsawra::model::{Action, Message};

/// One hit: who, when, and the text with the query in bold.
#[derive(Clone, Debug, PartialEq)]
struct Hit {
    chat: String,
    id: String,
    who: String,
    when: String,
    markup: String,
}

pub struct SearchPane {
    pub root: awra::Box,
    entry: awra::SearchEntry,
    status: awra::Label,
    list: awra::Box,
    shown: Vec<Hit>,
    driver: std::rc::Weak<Driver>,
}

fn escape(text: &str) -> String {
    awra::glib::markup_escape_text(text)
}

/// `text` with every match of `query` (ignoring case) in bold, cut to a
/// window around the first so a long message still shows where it matched.
fn emphasised(text: &str, query: &str) -> String {
    let text = text.replace('\n', " ");
    let query = query.trim();
    if query.is_empty() {
        return escape(&text);
    }
    let lower = text.to_lowercase();
    let needle = query.to_lowercase();
    // Lowercasing can change lengths: then only the plain text is shown.
    if lower.len() != text.len() {
        return escape(&text);
    }
    let start = lower.find(&needle).map_or(0, |at| at.saturating_sub(40));
    let start = (0..=start)
        .rev()
        .find(|at| text.is_char_boundary(*at))
        .unwrap_or(0);
    let mut out = String::new();
    if start > 0 {
        out.push('…');
    }
    let mut at = start;
    while let Some(found) = lower[at..].find(&needle) {
        let begin = at + found;
        let end = begin + needle.len();
        out.push_str(&escape(&text[at..begin]));
        out.push_str("<b>");
        out.push_str(&escape(&text[begin..end]));
        out.push_str("</b>");
        at = end;
    }
    out.push_str(&escape(&text[at..]));
    out
}

fn hit_of(app: &App, message: &Message) -> Hit {
    let who = if message.from_me {
        gettext(app.locale, "You").into_owned()
    } else {
        app.display_name_or(&message.sender, message.sender_name.as_deref())
    };
    Hit {
        chat: message.chat.clone(),
        id: message.id.clone(),
        who,
        when: whatsawra::util::moment_stamp(app.locale, message.timestamp),
        markup: emphasised(&app.message_text(message), &app.chat_search),
    }
}

impl SearchPane {
    pub fn build(driver: &Rc<Driver>, locale: Locale) -> Self {
        let t = |text: &'static str| gettext(locale, text).into_owned();
        let root = awra::Box::new(awra::Orientation::Vertical, 8);
        root.set_size_request(320, -1);
        root.set_margin_top(10);
        root.set_margin_start(12);
        root.set_margin_end(12);

        let top = awra::Box::new(awra::Orientation::Horizontal, 8);
        let title = awra::Label::new(Some(&t("Search messages")));
        title.set_xalign(0.0);
        title.set_hexpand(true);
        title.set_weight(600);
        let close = awra::Button::from_icon_name("window-close-symbolic");
        close.set_appearance(awra::ButtonAppearance::Toolbar);
        close.set_tooltip_text(Some(t("Close").as_str()));
        let weak = Rc::downgrade(driver);
        close.connect_clicked(move |_| {
            if let Some(driver) = weak.upgrade() {
                driver.act(Action::CloseChatSearch);
            }
        });
        top.append(&title);
        top.append(&close);
        root.append(&top);

        let entry = awra::SearchEntry::new();
        entry.set_placeholder_text(t("Search"));
        entry.set_search_delay(250);
        let weak = Rc::downgrade(driver);
        entry.connect_search_changed(move |entry| {
            let query = entry.text();
            if let Some(driver) = weak.upgrade()
                && driver.read(|app| app.chat_search != query)
            {
                driver.act(Action::ChatSearch(query));
            }
        });
        let weak = Rc::downgrade(driver);
        entry.connect_stop_search(move |_| {
            if let Some(driver) = weak.upgrade() {
                driver.act(Action::CloseChatSearch);
            }
        });
        root.append(&entry);

        let status = awra::Label::new(None);
        status.set_xalign(0.0);
        status.set_wrap(true);
        status.set_typography(awra::Typography::Muted);
        root.append(&status);

        let list = awra::Box::new(awra::Orientation::Vertical, 2);
        let scroll = awra::ScrolledWindow::new();
        scroll.set_hscrollbar_policy(awra::PolicyType::Never);
        scroll.set_child(Some(&list));
        scroll.set_vexpand(true);
        root.append(&scroll);

        Self {
            root,
            entry,
            status,
            list,
            shown: Vec::new(),
            driver: Rc::downgrade(driver),
        }
    }

    /// Fills the pane; `false` when it is closed.
    pub fn sync(&mut self, app: &App) -> bool {
        if !app.chat_search_visible() {
            if !self.shown.is_empty() {
                self.shown.clear();
                self.clear();
            }
            return false;
        }
        if self.entry.text() != app.chat_search {
            self.entry.set_text(&app.chat_search);
        }
        let locale = app.locale;
        let searching = !app.chat_search.trim().is_empty() || app.chat_search_day.is_some();
        let status = if !searching {
            gettext(locale, "Search for messages in this chat.").into_owned()
        } else if app.chat_search_pending && app.chat_search_hits.is_empty() {
            gettext(locale, "Searching…").into_owned()
        } else if app.chat_search_hits.is_empty() {
            gettext(locale, "No messages found").into_owned()
        } else if app.chat_search_truncated {
            gettext(
                locale,
                "Showing the newest matches. Add a word to narrow them down.",
            )
            .into_owned()
        } else {
            String::new()
        };
        self.status.set_visible(!status.is_empty());
        self.status.set_text(&status);

        let hits: Vec<Hit> = app
            .chat_search_hits
            .iter()
            .map(|message| hit_of(app, message))
            .collect();
        if hits != self.shown {
            self.clear();
            for hit in &hits {
                self.list.append(&self.row(hit));
            }
            self.shown = hits;
        }
        true
    }

    fn clear(&self) {
        while let Some(child) = self.list.first_child() {
            self.list.remove(&child);
        }
    }

    fn row(&self, hit: &Hit) -> awra::ToggleButton {
        let lines = awra::Box::new(awra::Orientation::Vertical, 2);
        let top = awra::Box::new(awra::Orientation::Horizontal, 8);
        let who = awra::Label::new(Some(&hit.who));
        who.set_xalign(0.0);
        who.set_hexpand(true);
        who.set_weight(600);
        who.set_ellipsize(awra::pango::EllipsizeMode::End);
        let when = awra::Label::new(Some(&hit.when));
        when.set_typography(awra::Typography::Muted);
        top.append(&who);
        top.append(&when);
        let text = awra::Label::new(None);
        text.set_markup(&hit.markup);
        text.set_xalign(0.0);
        text.set_wrap(true);
        text.set_lines(3);
        text.set_ellipsize(awra::pango::EllipsizeMode::End);
        lines.append(&top);
        lines.append(&text);
        let (weak, chat, id) = (self.driver.clone(), hit.chat.clone(), hit.id.clone());
        super::rows::button(&lines, false, move || {
            if let Some(driver) = weak.upgrade() {
                driver.act(Action::OpenMessage {
                    chat: chat.clone(),
                    message: id.clone(),
                });
            }
        })
    }

    /// Puts the keyboard in the field.
    pub fn focus(&self) {
        self.entry.grab_focus();
    }
}

#[cfg(test)]
mod tests {
    use super::emphasised;

    #[test]
    fn the_query_is_bold_wherever_it_matches() {
        assert_eq!(
            emphasised("The Engine and the engine", "engine"),
            "The <b>Engine</b> and the <b>engine</b>"
        );
        assert_eq!(emphasised("a < b", "b"), "a &lt; <b>b</b>");
    }

    #[test]
    fn a_long_text_shows_where_it_matched() {
        let text = format!("{} needle", "x".repeat(100));
        let shown = emphasised(&text, "needle");
        assert!(shown.starts_with('…'));
        assert!(shown.ends_with("<b>needle</b>"));
    }
}
