//! The app's dialogs as Awra dialogs: `App::dialog` says which one is open,
//! and this view opens, replaces or closes the Awra dialog to match. Its
//! buttons queue the dialog's actions; closing it queues `CloseDialog`.

use std::cell::Cell;
use std::rc::Rc;

use super::Driver;
use whatsawra::app::App;
use whatsawra::i18n::gettext;
use whatsawra::model::{Action, Dialog};

pub struct Dialogs {
    shown: Option<(Dialog, awra::Dialog)>,
    /// The open dialog when it reads the app at every turn.
    chooser: Option<super::chooser::Chooser>,
}

impl Dialogs {
    pub fn new() -> Self {
        Self {
            shown: None,
            chooser: None,
        }
    }

    /// Closes the open dialog without answering it (the lock hides it; the
    /// app keeps it, and it opens again once unlocked).
    pub fn close(&mut self) {
        self.chooser = None;
        if let Some((_, old)) = self.shown.take() {
            old.set_rust_data("closed-by-app", true);
            old.close();
        }
    }

    pub fn sync(&mut self, driver: &Rc<Driver>, app: &App, window: &awra::Window) {
        if self.shown.as_ref().map(|(dialog, _)| dialog) == app.dialog.as_ref() {
            if let Some(chooser) = &mut self.chooser {
                chooser.sync(driver, app);
            }
            return;
        }
        self.chooser = None;
        if let Some((_, old)) = self.shown.take() {
            // Closed by the app, not the reader: say nothing back.
            old.set_rust_data("closed-by-app", true);
            old.close();
        }
        let Some(dialog) = app.dialog.clone() else {
            return;
        };
        let shown = if let Some(confirmation) = whatsawra::confirm::of(app, &dialog) {
            confirmation_dialog(driver, app, window, confirmation)
        } else if let Some(mut chooser) = super::chooser::Chooser::of(driver, app, window, &dialog)
        {
            chooser.sync(driver, app);
            let shown = chooser.dialog.clone();
            self.chooser = Some(chooser);
            shown
        } else {
            not_yet(driver, app, window)
        };
        shown.present();
        self.shown = Some((dialog, shown));
    }
}

/// Queues `CloseDialog` when the reader closes `dialog`, unless the app
/// closed it or a button already answered.
pub(super) fn close_on_dismiss(
    driver: &Rc<Driver>,
    dialog: &awra::Dialog,
    answered: Rc<Cell<bool>>,
) {
    let weak = Rc::downgrade(driver);
    dialog.connect_close_request(move |dialog| {
        let by_app = dialog.rust_data::<bool>("closed-by-app").unwrap_or(false);
        if !by_app
            && !answered.get()
            && let Some(driver) = weak.upgrade()
        {
            driver.act(Action::CloseDialog);
        }
        awra::glib::Propagation::Proceed
    });
}

fn confirmation_dialog(
    driver: &Rc<Driver>,
    app: &App,
    window: &awra::Window,
    confirmation: whatsawra::confirm::Confirmation,
) -> awra::Dialog {
    let dialog = awra::Dialog::new(Some(window));
    dialog.set_title(Some(confirmation.heading.as_str()));
    dialog.set_body_text(&confirmation.body.join("\n\n"));
    dialog.set_content_width(420);
    dialog.add_response("cancel", gettext(app.locale, "Cancel").as_ref());
    for (index, choice) in confirmation.choices.iter().enumerate() {
        let id = index.to_string();
        dialog.add_response(&id, &choice.label);
        dialog.set_response_appearance(
            &id,
            if choice.destructive {
                awra::ButtonAppearance::Destructive
            } else {
                awra::ButtonAppearance::Secondary
            },
        );
        dialog.set_response_enabled(&id, confirmation.enabled);
    }
    // Enter is not bound: a stray key must not destroy anything.
    dialog.set_close_response(Some("cancel"));
    let answered = Rc::new(Cell::new(false));
    let weak = Rc::downgrade(driver);
    let choices = confirmation.choices;
    let replied = answered.clone();
    dialog.connect_response(None, move |_, response| {
        let Some(driver) = weak.upgrade() else { return };
        replied.set(true);
        match response
            .parse::<usize>()
            .ok()
            .and_then(|index| choices.get(index))
        {
            Some(choice) => {
                for action in &choice.actions {
                    driver.act(action.clone());
                }
                if choice.closes {
                    driver.act(Action::CloseDialog);
                }
            }
            None => driver.act(Action::CloseDialog),
        }
    });
    close_on_dismiss(driver, &dialog, answered);
    dialog
}

/// A dialog this window does not draw yet: says so, and closes.
fn not_yet(driver: &Rc<Driver>, app: &App, window: &awra::Window) -> awra::Dialog {
    let dialog = awra::Dialog::new(Some(window));
    dialog.set_title(Some("WhatsAwra"));
    dialog.set_body_text(
        "This part of WhatsAwra is not in the Awra window yet. The egui window (whatsawra) has it.",
    );
    dialog.add_response("close", gettext(app.locale, "Close").as_ref());
    dialog.set_close_response(Some("close"));
    let answered = Rc::new(Cell::new(false));
    let weak = Rc::downgrade(driver);
    let replied = answered.clone();
    dialog.connect_response(None, move |_, _| {
        replied.set(true);
        if let Some(driver) = weak.upgrade() {
            driver.act(Action::CloseDialog);
        }
    });
    close_on_dismiss(driver, &dialog, answered);
    dialog
}
