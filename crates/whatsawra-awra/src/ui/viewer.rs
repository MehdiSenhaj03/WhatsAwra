//! The picture viewer: the open picture over the whole window, fitted or
//! at a zoom, with the chat's other pictures and clips a step away, as the
//! egui viewer browses them. Its state is the app's (`App::image_preview`).

use std::rc::Rc;

use awra::prelude::*;

use super::Driver;
use whatsawra::app::App;
use whatsawra::i18n::{Locale, gettext};
use whatsawra::model::Action;

pub struct Viewer {
    pub root: awra::Box,
    image: awra::Image,
    scroller: awra::ScrolledWindow,
    position: awra::Label,
    previous: awra::Button,
    next: awra::Button,
    zoom: awra::Label,
    missing: awra::EmptyState,
    shown: Option<(std::path::PathBuf, i32)>,
}

fn tool(icon: &str, tooltip: &str, driver: &Rc<Driver>, action: Action) -> awra::Button {
    let button = awra::Button::from_icon_name(icon);
    button.set_appearance(awra::ButtonAppearance::Toolbar);
    button.set_tooltip_text(Some(tooltip));
    let weak = Rc::downgrade(driver);
    button.connect_clicked(move |_| {
        if let Some(driver) = weak.upgrade() {
            driver.act(action.clone());
        }
    });
    button
}

impl Viewer {
    pub fn build(driver: &Rc<Driver>, locale: Locale) -> Self {
        let t = |text: &'static str| gettext(locale, text).into_owned();
        let root = awra::Box::new(awra::Orientation::Vertical, 0);

        let bar = awra::Box::new(awra::Orientation::Horizontal, 6);
        bar.set_margin_top(8);
        bar.set_margin_bottom(8);
        bar.set_margin_start(12);
        bar.set_margin_end(12);
        let close = tool(
            "go-previous-symbolic",
            &t("Close"),
            driver,
            Action::CloseImagePreview,
        );
        bar.append(&close);
        let position = awra::Label::new(None);
        position.set_hexpand(true);
        position.set_xalign(0.0);
        position.set_typography(awra::Typography::Muted);
        bar.append(&position);
        let previous = tool(
            "go-previous-symbolic",
            &t("Previous"),
            driver,
            Action::ViewerStep(-1),
        );
        let next = tool(
            "go-next-symbolic",
            &t("Next"),
            driver,
            Action::ViewerStep(1),
        );
        bar.append(&previous);
        bar.append(&next);
        bar.append(&tool(
            "zoom-out-symbolic",
            &t("Zoom out"),
            driver,
            Action::ZoomImageOut,
        ));
        let zoom = awra::Label::new(None);
        zoom.set_tabular_digits(true);
        zoom.set_width_chars(5);
        bar.append(&zoom);
        bar.append(&tool(
            "zoom-in-symbolic",
            &t("Zoom in"),
            driver,
            Action::ZoomImageIn,
        ));
        bar.append(&tool(
            "zoom-original-symbolic",
            &t("Fit to window"),
            driver,
            Action::FitImage,
        ));
        root.append(&bar);

        let image = awra::Image::new();
        image.set_content_fit(awra::ContentFit::Contain);
        image.set_hexpand(true);
        image.set_vexpand(true);
        let scroller = awra::ScrolledWindow::new();
        scroller.set_child(Some(&image));
        scroller.set_vexpand(true);
        root.append(&scroller);
        let missing = awra::EmptyState::new();
        missing.set_icon_name(Some("image-x-generic-symbolic"));
        missing.set_title(Some(t("Not downloaded").as_str()));
        missing.set_vexpand(true);
        root.append(&missing);

        // Escape closes, the arrows step through the chat's pictures.
        let keys = awra::EventControllerKey::new();
        let weak = Rc::downgrade(driver);
        keys.connect_key_pressed(move |_, key, _, _| {
            let action = if key == awra::gdk::Key::Escape {
                Action::CloseImagePreview
            } else if key == awra::gdk::Key::Left {
                Action::ViewerStep(-1)
            } else if key == awra::gdk::Key::Right {
                Action::ViewerStep(1)
            } else {
                return awra::glib::Propagation::Proceed;
            };
            if let Some(driver) = weak.upgrade() {
                driver.act(action);
            }
            awra::glib::Propagation::Stop
        });
        root.add_controller(keys);
        root.set_focusable(true);

        Self {
            root,
            image,
            scroller,
            position,
            previous,
            next,
            zoom,
            missing,
            shown: None,
        }
    }

    /// Shows the open picture; `false` when none is open.
    pub fn sync(&mut self, app: &App) -> bool {
        let Some(preview) = app.image_preview.as_ref() else {
            self.shown = None;
            return false;
        };
        let album = &app.viewer_media;
        let at = preview.position(album);
        self.position.set_text(&match at {
            Some(at) if album.len() > 1 => format!("{} / {}", at + 1, album.len()),
            _ => String::new(),
        });
        self.previous.set_sensitive(at.is_some_and(|at| at > 0));
        self.next
            .set_sensitive(at.is_some_and(|at| at + 1 < album.len()));
        self.zoom.set_text(&if preview.is_fit() {
            gettext(app.locale, "Fit").into_owned()
        } else {
            format!("{:.0}%", preview.zoom() * 100.0)
        });
        let path = preview.path().map(std::path::Path::to_path_buf);
        self.missing.set_visible(path.is_none());
        self.scroller.set_visible(path.is_some());
        let Some(path) = path else {
            self.shown = None;
            return true;
        };
        let texture = super::pictures::full(&path);
        // At a zoom the picture takes its size in pixels times the zoom,
        // and the scroller pans it; fitted, it fills the room.
        let zoom = if preview.is_fit() {
            0
        } else {
            (preview.zoom() * 100.0) as i32
        };
        if self.shown.as_ref() != Some(&(path.clone(), zoom))
            && let Some(texture) = texture
        {
            self.image.set_texture(Some(&texture));
            match zoom {
                zoom if zoom > 0 => {
                    self.image.set_size_request(
                        texture.width() * zoom / 100,
                        texture.height() * zoom / 100,
                    );
                    self.image.set_content_fit(awra::ContentFit::Fill);
                }
                _ => {
                    self.image.set_size_request(-1, -1);
                    self.image.set_content_fit(awra::ContentFit::Contain);
                }
            }
            // Fitted, the picture is as large as the room; zoomed, it pans.
            if zoom > 0 {
                self.scroller
                    .set_policy(awra::PolicyType::Automatic, awra::PolicyType::Automatic);
            } else {
                self.scroller
                    .set_policy(awra::PolicyType::Never, awra::PolicyType::Never);
            }
            self.shown = Some((path, zoom));
        }
        true
    }

    /// Puts the keyboard on the viewer, for its keys.
    pub fn focus(&self) {
        self.root.grab_focus();
    }
}
