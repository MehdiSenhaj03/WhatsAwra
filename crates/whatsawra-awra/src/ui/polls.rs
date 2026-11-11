//! Polls: the ballot inside a poll's bubble, its results, and the dialog
//! that makes one. Votes go through `Action::VotePoll`, as in the egui
//! window, with the same rule for what a click selects.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use awra::prelude::*;

use super::Driver;
use super::dialogs::Live;
use whatsawra::app::App;
use whatsawra::i18n::{Locale, gettext, ngettext};
use whatsawra::model::{Action, Content, Dialog, Message, PollState};

/// What a poll's bubble shows.
#[derive(Clone, Debug, PartialEq)]
pub struct PollView {
    chat: String,
    question: String,
    hint: String,
    /// Each option: its text, votes, ours, and its share of the voters.
    options: Vec<(String, usize, bool, f64)>,
    detail: String,
    note: Option<String>,
    /// Clicks vote (connected, a key to vote with, no vote on its way).
    active: bool,
    /// The phone should be asked for earlier votes.
    refresh: bool,
    state: PollState,
}

pub fn view(app: &App, message: &Message) -> Option<PollView> {
    let Content::Poll {
        question,
        options,
        state,
    } = &message.content
    else {
        return None;
    };
    let locale = app.locale;
    let t = |text: &'static str| gettext(locale, text).into_owned();
    let enabled = app.is_connected();
    let pending = app
        .poll_voting
        .contains(&(message.chat.clone(), message.id.clone()));
    let detail = if pending {
        t("Sending vote…")
    } else if state.refresh_failed {
        t("Waiting for your phone · earlier votes may be missing")
    } else if state.refreshing && !state.history_complete {
        t("Loading earlier votes from your phone…")
    } else if !state.history_complete {
        t("Earlier votes have not been loaded yet")
    } else {
        ngettext(locale, "{} voter", "{} voters", state.voters as u32)
            .replace("{}", &state.voters.to_string())
    };
    let note = if !state.can_vote {
        Some(t("Voting key unavailable · use your phone"))
    } else if !enabled {
        Some(t("Reconnect to vote"))
    } else {
        None
    };
    Some(PollView {
        chat: message.chat.clone(),
        question: question.clone(),
        hint: t(if state.selectable == 1 {
            "Select one answer"
        } else {
            "Select answers"
        }),
        options: options
            .iter()
            .enumerate()
            .map(|(index, option)| {
                let count = state.counts.get(index).copied().unwrap_or_default();
                let share = if state.voters > 0 {
                    (count as f64 / state.voters as f64).min(1.0)
                } else {
                    0.0
                };
                (
                    option.clone(),
                    count,
                    state.selected.contains(&index),
                    share,
                )
            })
            .collect(),
        detail,
        note,
        active: enabled && state.can_vote && !pending,
        refresh: enabled && state.refresh_needed && !state.refreshing,
        state: state.clone(),
    })
}

struct OptionRow {
    button: awra::Button,
    check: awra::CheckButton,
    label: awra::Label,
    count: awra::Label,
    bar: awra::ProgressBar,
}

/// The ballot's widgets, made once per message row.
#[derive(Clone)]
pub struct PollWidgets {
    pub root: awra::Box,
    question: awra::Label,
    hint: awra::Label,
    options: awra::Box,
    rows: Rc<RefCell<Vec<OptionRow>>>,
    detail: awra::Label,
    note: awra::Label,
    shown: Rc<RefCell<Option<(String, PollView)>>>,
    /// Messages whose earlier votes this row has asked the phone for.
    asked: Rc<RefCell<std::collections::HashSet<String>>>,
    driver: Weak<Driver>,
}

/// A dialog's content, kept off its edges.
fn inset(widget: &impl awra::IsWidget) {
    let widget = widget.upcast_ref();
    widget.set_margin_start(16);
    widget.set_margin_end(16);
    widget.set_margin_bottom(8);
}

fn muted(size: bool) -> awra::Label {
    let label = awra::Label::new(None);
    label.set_xalign(0.0);
    label.set_wrap(true);
    label.set_typography(awra::Typography::Muted);
    if size {
        label.set_scale(0.9);
    }
    label
}

impl PollWidgets {
    pub fn build(driver: &Weak<Driver>, locale: Locale) -> Self {
        let root = awra::Box::new(awra::Orientation::Vertical, 4);
        root.set_size_request(280, -1);
        let question = awra::Label::new(None);
        question.set_xalign(0.0);
        question.set_wrap(true);
        question.set_weight(600);
        root.append(&question);
        let hint = muted(true);
        root.append(&hint);
        let options = awra::Box::new(awra::Orientation::Vertical, 2);
        root.append(&options);
        let detail = muted(true);
        root.append(&detail);
        let note = muted(true);
        root.append(&note);
        let results = awra::Button::with_label(&gettext(locale, "View votes"));
        results.set_appearance(awra::ButtonAppearance::Ghost);
        root.append(&results);
        let shown: Rc<RefCell<Option<(String, PollView)>>> = Rc::default();
        let (weak, poll) = (driver.clone(), shown.clone());
        results.connect_clicked(move |_| {
            let Some(driver) = weak.upgrade() else { return };
            if let Some((id, view)) = poll.borrow().clone() {
                driver.act(Action::ShowDialog(Dialog::PollResults {
                    chat: view.chat,
                    message: id,
                }));
            }
        });
        Self {
            root,
            question,
            hint,
            options,
            rows: Rc::default(),
            detail,
            note,
            shown,
            asked: Rc::default(),
            driver: driver.clone(),
        }
    }

    fn row(&self, index: usize) -> OptionRow {
        let button = awra::Button::new();
        button.set_appearance(awra::ButtonAppearance::Ghost);
        let lines = awra::Box::new(awra::Orientation::Vertical, 4);
        let top = awra::Box::new(awra::Orientation::Horizontal, 8);
        let check = awra::CheckButton::new();
        check.set_can_target(false);
        let label = awra::Label::new(None);
        label.set_xalign(0.0);
        label.set_wrap(true);
        label.set_hexpand(true);
        let count = muted(false);
        count.set_tabular_digits(true);
        top.append(&check);
        top.append(&label);
        top.append(&count);
        let bar = awra::ProgressBar::new();
        lines.append(&top);
        lines.append(&bar);
        button.set_child(Some(&lines));
        let (weak, shown) = (self.driver.clone(), self.shown.clone());
        button.connect_clicked(move |_| {
            let Some(driver) = weak.upgrade() else { return };
            let Some((id, view)) = shown.borrow().clone() else {
                return;
            };
            if !view.active {
                return;
            }
            if let Some(choices) = whatsawra::ui::polls::selection_after_click(&view.state, index) {
                driver.act(Action::VotePoll {
                    chat: view.chat,
                    message: id,
                    choices,
                });
            }
        });
        OptionRow {
            button,
            check,
            label,
            count,
            bar,
        }
    }

    pub fn bind(&self, message: &str, view: Option<&PollView>) {
        let Some(view) = view else {
            self.root.set_visible(false);
            *self.shown.borrow_mut() = None;
            return;
        };
        self.root.set_visible(true);
        self.question.set_text(&view.question);
        self.hint.set_text(&view.hint);
        let mut rows = self.rows.borrow_mut();
        while rows.len() < view.options.len() {
            let row = self.row(rows.len());
            self.options.append(&row.button);
            rows.push(row);
        }
        for (index, row) in rows.iter().enumerate() {
            let Some((text, count, ours, share)) = view.options.get(index) else {
                row.button.set_visible(false);
                continue;
            };
            row.button.set_visible(true);
            row.button.set_sensitive(view.active);
            row.check.set_active(*ours);
            row.label.set_text(text);
            row.count.set_text(&count.to_string());
            row.bar.set_fraction(*share);
        }
        self.detail.set_text(&view.detail);
        self.note.set_visible(view.note.is_some());
        self.note.set_text(view.note.as_deref().unwrap_or_default());
        *self.shown.borrow_mut() = Some((message.to_owned(), view.clone()));
        if view.refresh
            && self.asked.borrow_mut().insert(message.to_owned())
            && let Some(driver) = self.driver.upgrade()
        {
            driver.later(super::Op::Action(Action::RefreshPoll {
                chat: view.chat.clone(),
                message: message.to_owned(),
            }));
        }
    }
}

/// The dialogs this module draws, for `dialog`.
pub fn dialog_of(
    driver: &Rc<Driver>,
    app: &App,
    window: &awra::Window,
    dialog: &Dialog,
) -> Option<Box<dyn Live>> {
    match dialog {
        Dialog::CreatePoll(chat) => Some(Box::new(Create::new(driver, app, window, chat))),
        Dialog::PollResults { chat, message } => {
            Some(Box::new(Results::new(driver, app, window, chat, message)))
        }
        _ => None,
    }
}

/// Create poll: a question, two to twelve answers, one or several choices.
struct Create {
    dialog: awra::Dialog,
    chat: String,
    question: awra::Entry,
    answers: awra::Box,
    entries: Vec<awra::Entry>,
    add: awra::Button,
    multiple: awra::Switch,
    problem: awra::Label,
    driver: Weak<Driver>,
}

impl Create {
    fn new(driver: &Rc<Driver>, app: &App, window: &awra::Window, chat: &str) -> Self {
        let t = |text: &'static str| gettext(app.locale, text).into_owned();
        let dialog = awra::Dialog::new(Some(window));
        dialog.set_title(Some(t("Create poll").as_str()));
        dialog.set_content_width(420);
        let content = awra::Box::new(awra::Orientation::Vertical, 8);
        inset(&content);
        let heading = |text: String| {
            let label = awra::Label::new(Some(&text));
            label.set_xalign(0.0);
            label.set_weight(600);
            label
        };
        content.append(&heading(t("Question")));
        let question = awra::Entry::new();
        question.set_placeholder_text(t("Ask a question"));
        let weak = Rc::downgrade(driver);
        question.connect_changed(move |entry| {
            let text = entry.text();
            if let Some(driver) = weak.upgrade() {
                driver.edit(move |app| app.poll_draft.question = text);
            }
        });
        content.append(&question);
        content.append(&heading(t("Answers")));
        let answers = awra::Box::new(awra::Orientation::Vertical, 6);
        content.append(&answers);
        let add = awra::Button::with_label(&t("Add answer"));
        add.set_halign(awra::Align::Start);
        let weak = Rc::downgrade(driver);
        add.connect_clicked(move |_| {
            if let Some(driver) = weak.upgrade() {
                driver.edit(|app| {
                    if app.poll_draft.options.len() < 12 {
                        app.poll_draft.options.push(String::new());
                    }
                });
            }
        });
        content.append(&add);
        let line = awra::Box::new(awra::Orientation::Horizontal, 10);
        let multiple = awra::Switch::new();
        let weak = Rc::downgrade(driver);
        multiple.connect_active_notify(move |switch| {
            let on = switch.is_active();
            if let Some(driver) = weak.upgrade() {
                driver.edit(move |app| app.poll_draft.multiple = on);
            }
        });
        let label = awra::Label::new(Some(&t("Allow multiple answers")));
        label.set_hexpand(true);
        label.set_xalign(0.0);
        line.append(&label);
        line.append(&multiple);
        content.append(&line);
        let problem = muted(true);
        content.append(&problem);
        dialog.set_child(Some(&content));
        dialog.add_response("cancel", &t("Cancel"));
        dialog.add_response("send", &t("Send poll"));
        dialog.set_response_appearance("send", awra::ButtonAppearance::Primary);
        dialog.set_response_closes("send", false);
        dialog.set_close_response(Some("cancel"));
        let answered = Rc::new(Cell::new(false));
        let (weak, replied, to) = (Rc::downgrade(driver), answered.clone(), chat.to_owned());
        dialog.connect_response(None, move |_, response| {
            let Some(driver) = weak.upgrade() else { return };
            if response == "send" {
                let draft = driver.read(|app| app.poll_draft.clone());
                driver.act(Action::CreatePoll {
                    chat: to.clone(),
                    draft,
                });
            } else {
                replied.set(true);
                driver.act(Action::CloseDialog);
            }
        });
        super::dialogs::close_on_dismiss(driver, &dialog, answered);
        Self {
            dialog,
            chat: chat.to_owned(),
            question,
            answers,
            entries: Vec::new(),
            add,
            multiple,
            problem,
            driver: Rc::downgrade(driver),
        }
    }
}

impl Live for Create {
    fn dialog(&self) -> awra::Dialog {
        self.dialog.clone()
    }

    fn sync(&mut self, _driver: &Rc<Driver>, app: &App) {
        let draft = &app.poll_draft;
        let locale = app.locale;
        if self.question.text() != draft.question {
            self.question.set_text(&draft.question);
        }
        while self.entries.len() < draft.options.len() {
            let index = self.entries.len();
            let entry = awra::Entry::new();
            entry.set_placeholder_text(
                gettext(locale, "Answer {}").replace("{}", &(index + 1).to_string()),
            );
            let weak = self.driver.clone();
            entry.connect_changed(move |entry| {
                let text = entry.text();
                if let Some(driver) = weak.upgrade() {
                    driver.edit(move |app| {
                        if let Some(option) = app.poll_draft.options.get_mut(index) {
                            *option = text;
                        }
                    });
                }
            });
            self.answers.append(&entry);
            self.entries.push(entry);
        }
        for (index, entry) in self.entries.iter().enumerate() {
            match draft.options.get(index) {
                Some(text) => {
                    entry.set_visible(true);
                    if entry.text() != *text {
                        entry.set_text(text);
                    }
                }
                None => entry.set_visible(false),
            }
        }
        self.add.set_visible(draft.options.len() < 12);
        if self.multiple.is_active() != draft.multiple {
            self.multiple.set_active(draft.multiple);
        }
        let valid = draft.validated();
        let problem = match &valid {
            Err(error) => gettext(locale, error).into_owned(),
            Ok(_) if !app.is_connected() => gettext(locale, "Reconnect to vote").into_owned(),
            Ok(_) => String::new(),
        };
        self.problem.set_visible(!problem.is_empty());
        self.problem.set_text(&problem);
        let sendable = valid.is_ok() && !app.poll_creating && app.is_connected();
        self.dialog.set_response_enabled("send", sendable);
        let _ = &self.chat;
    }
}

/// Who voted for what.
struct Results {
    dialog: awra::Dialog,
    chat: String,
    message: String,
    list: awra::Box,
    shown: Option<(Vec<String>, PollState)>,
}

impl Results {
    fn new(
        driver: &Rc<Driver>,
        app: &App,
        window: &awra::Window,
        chat: &str,
        message: &str,
    ) -> Self {
        let t = |text: &'static str| gettext(app.locale, text).into_owned();
        let dialog = awra::Dialog::new(Some(window));
        dialog.set_title(Some(t("Poll results").as_str()));
        dialog.set_content_width(420);
        dialog.set_default_size(440, 560);
        let list = awra::Box::new(awra::Orientation::Vertical, 6);
        let scrolled = awra::ScrolledWindow::new();
        scrolled.set_hscrollbar_policy(awra::PolicyType::Never);
        scrolled.set_child(Some(&list));
        scrolled.set_vexpand(true);
        inset(&scrolled);
        scrolled.set_min_content_height(320);
        dialog.set_child(Some(&scrolled));
        dialog.add_response("close", &t("Close"));
        dialog.set_close_response(Some("close"));
        let answered = Rc::new(Cell::new(false));
        let (weak, replied) = (Rc::downgrade(driver), answered.clone());
        dialog.connect_response(None, move |_, _| {
            replied.set(true);
            if let Some(driver) = weak.upgrade() {
                driver.act(Action::CloseDialog);
            }
        });
        super::dialogs::close_on_dismiss(driver, &dialog, answered);
        Self {
            dialog,
            chat: chat.to_owned(),
            message: message.to_owned(),
            list,
            shown: None,
        }
    }
}

impl Live for Results {
    fn dialog(&self) -> awra::Dialog {
        self.dialog.clone()
    }

    fn sync(&mut self, _driver: &Rc<Driver>, app: &App) {
        let locale = app.locale;
        let poll = app
            .conversations
            .get(&self.chat)
            .and_then(|conversation| conversation.message(&self.message))
            .and_then(|message| match &message.content {
                Content::Poll {
                    question,
                    options,
                    state,
                } => Some((question.clone(), options.clone(), state.clone())),
                _ => None,
            });
        let key = poll
            .as_ref()
            .map(|(question, options, state)| {
                let mut texts = options.clone();
                texts.push(question.clone());
                (texts, state.clone())
            })
            .unwrap_or_default();
        if self.shown.as_ref() == Some(&key) {
            return;
        }
        while let Some(child) = self.list.first_child() {
            self.list.remove(&child);
        }
        let Some((question, options, state)) = poll else {
            let gone = muted(false);
            gone.set_text(&gettext(locale, "This poll is no longer available."));
            self.list.append(&gone);
            self.shown = Some(key);
            return;
        };
        let title = awra::Label::new(Some(&question));
        title.set_xalign(0.0);
        title.set_wrap(true);
        title.set_weight(600);
        title.set_scale(1.15);
        self.list.append(&title);
        if !state.history_complete {
            let note = muted(true);
            note.set_text(&gettext(
                locale,
                "Earlier votes may still be missing. Results update as they arrive.",
            ));
            self.list.append(&note);
        }
        for (index, option) in options.iter().enumerate() {
            let count = state.counts.get(index).copied().unwrap_or(0);
            let heading = awra::Box::new(awra::Orientation::Horizontal, 8);
            heading.set_margin_top(12);
            let name = awra::Label::new(Some(option));
            name.set_xalign(0.0);
            name.set_wrap(true);
            name.set_hexpand(true);
            name.set_weight(600);
            let votes = muted(false);
            votes.set_text(
                &ngettext(locale, "{} vote", "{} votes", count as u32)
                    .replace("{}", &count.to_string()),
            );
            heading.append(&name);
            heading.append(&votes);
            self.list.append(&heading);
            for voter in state
                .votes
                .iter()
                .filter(|vote| vote.choices.contains(&index))
            {
                let row = awra::Box::new(awra::Orientation::Horizontal, 10);
                let face = awra::Avatar::new(32, Some(voter.name.as_str()));
                let picture = app
                    .avatar_path(&voter.id)
                    .and_then(|path| super::pictures::file(&path));
                face.set_custom_image(picture.as_ref());
                let lines = awra::Box::new(awra::Orientation::Vertical, 0);
                let who = awra::Label::new(Some(&voter.name));
                who.set_xalign(0.0);
                let when = muted(true);
                when.set_text(&whatsawra::util::moment_stamp(locale, voter.timestamp));
                lines.append(&who);
                lines.append(&when);
                row.append(&face);
                row.append(&lines);
                self.list.append(&row);
            }
        }
        self.shown = Some(key);
    }
}
