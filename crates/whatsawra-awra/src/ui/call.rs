//! The call screen: who, the call's state in the egui screen's words, the
//! peer's picture and ours on a video call, and the controls (answer and
//! decline while it rings; mute, camera and hang up once up). It covers the
//! window while a call is on, as the egui screen does.

use std::rc::Rc;
use std::sync::Arc;

use awra::prelude::*;

use super::Driver;
use whatsawra::app::App;
use whatsawra::calls::CallPhase;
use whatsawra::i18n::{Locale, gettext};
use whatsawra::model::Action;

/// The widest our own picture is drawn, in a corner.
const LOCAL_WIDTH: usize = 220;

pub struct CallScreen {
    pub root: awra::Overlay,
    remote: awra::Image,
    local: awra::Image,
    avatar: awra::Avatar,
    name: awra::Label,
    status: awra::Label,
    ringing: awra::Box,
    bar: awra::Widget,
    mute: awra::ToggleButton,
    camera: awra::ToggleButton,
    shown_remote: Option<usize>,
    shown_local: Option<usize>,
    shown_chat: Option<String>,
}

fn round_button(icon: &str, label: &str, appearance: awra::ButtonAppearance) -> awra::Box {
    let column = awra::Box::new(awra::Orientation::Vertical, 6);
    let button = awra::Button::from_icon_name(icon);
    button.set_appearance(appearance);
    button.set_size_request(56, 56);
    button.set_halign(awra::Align::Center);
    button.set_tooltip_text(Some(label));
    column.append(&button);
    let text = awra::Label::new(Some(label));
    text.set_typography(awra::Typography::Muted);
    column.append(&text);
    column
}

fn on_click(column: &awra::Box, driver: &Rc<Driver>, action: Action) {
    if let Some(button) = column
        .first_child()
        .and_then(|child| child.downcast::<awra::Button>().ok())
    {
        let weak = Rc::downgrade(driver);
        button.connect_clicked(move |_| {
            if let Some(driver) = weak.upgrade() {
                driver.act(action.clone());
            }
        });
    }
}

impl CallScreen {
    pub fn build(driver: &Rc<Driver>, locale: Locale) -> Self {
        let t = |text: &'static str| gettext(locale, text).into_owned();
        let root = awra::Overlay::new();

        // The peer's picture fills the screen on a video call.
        let remote = awra::Image::new();
        remote.set_content_fit(awra::ContentFit::Contain);
        remote.set_hexpand(true);
        remote.set_vexpand(true);
        root.set_child(Some(&remote));

        let column = awra::Box::new(awra::Orientation::Vertical, 12);
        column.set_halign(awra::Align::Center);
        column.set_valign(awra::Align::Fill);
        column.set_margin_top(48);
        column.set_margin_bottom(36);
        let avatar = awra::Avatar::new(120, None);
        avatar.set_halign(awra::Align::Center);
        let name = awra::Label::new(None);
        name.set_typography(awra::Typography::Title1);
        let status = awra::Label::new(None);
        status.set_typography(awra::Typography::Muted);
        status.set_tabular_digits(true);
        column.append(&avatar);
        column.append(&name);
        column.append(&status);
        let spacer = awra::Box::new(awra::Orientation::Vertical, 0);
        spacer.set_vexpand(true);
        column.append(&spacer);

        let ringing = awra::Box::new(awra::Orientation::Horizontal, 48);
        ringing.set_halign(awra::Align::Center);
        let decline = round_button(
            "awra-call-end-symbolic",
            &t("Decline"),
            awra::ButtonAppearance::Destructive,
        );
        let accept = round_button(
            "awra-call-symbolic",
            &t("Accept"),
            awra::ButtonAppearance::Success,
        );
        on_click(&decline, driver, Action::DeclineCall);
        on_click(&accept, driver, Action::AnswerCall);
        ringing.append(&decline);
        ringing.append(&accept);
        column.append(&ringing);

        let live = awra::Box::new(awra::Orientation::Horizontal, 24);
        live.set_halign(awra::Align::Center);
        let mute = awra::ToggleButton::new();
        mute.set_icon_name(Some("awra-microphone-symbolic"));
        mute.set_tooltip_text(Some(t("Mute").as_str()));
        mute.set_size_request(56, 56);
        awra::add_style_role(&mute, "danger-when-on");
        let weak = Rc::downgrade(driver);
        mute.connect_toggled(move |button| {
            if let Some(driver) = weak.upgrade() {
                driver.act(Action::SetCallMuted(button.is_active()));
            }
        });
        let camera = awra::ToggleButton::new();
        camera.set_icon_name(Some("awra-video-call-symbolic"));
        camera.set_tooltip_text(Some(t("Camera").as_str()));
        camera.set_size_request(56, 56);
        let weak = Rc::downgrade(driver);
        camera.connect_toggled(move |button| {
            if let Some(driver) = weak.upgrade() {
                driver.act(Action::SetCallCamera(button.is_active()));
            }
        });
        let hangup = awra::Button::from_icon_name("awra-call-end-symbolic");
        hangup.set_appearance(awra::ButtonAppearance::Destructive);
        hangup.set_size_request(56, 56);
        hangup.set_tooltip_text(Some(t("Hang up").as_str()));
        let weak = Rc::downgrade(driver);
        hangup.connect_clicked(move |_| {
            if let Some(driver) = weak.upgrade() {
                driver.act(Action::HangupCall);
            }
        });
        let back = awra::Button::from_icon_name("go-previous-symbolic");
        back.set_size_request(56, 56);
        back.set_tooltip_text(Some(t("Chats").as_str()));
        let weak = Rc::downgrade(driver);
        back.connect_clicked(move |_| {
            if let Some(driver) = weak.upgrade() {
                driver.act(Action::LeaveCallSurface);
            }
        });
        live.append(&back);
        live.append(&mute);
        live.append(&camera);
        live.append(&hangup);
        // On a floating surface, so the controls read over the picture.
        let bar = awra::Surface::new(awra::Material::Floating);
        bar.set_radius(36.0);
        bar.set_padding(10, 16, 10, 16);
        bar.set_halign(awra::Align::Center);
        bar.set_child(Some(&live));
        column.append(&bar);
        root.add_overlay(&column);

        // Our own picture, small, in a corner.
        let local = awra::Image::new();
        local.set_size_request(200, 150);
        local.set_corner_radius(12.0);
        local.set_content_fit(awra::ContentFit::Cover);
        local.set_halign(awra::Align::End);
        local.set_valign(awra::Align::Start);
        local.set_margin_top(16);
        local.set_margin_end(16);
        root.add_overlay(&local);

        Self {
            root,
            remote,
            local,
            avatar,
            name,
            status,
            ringing,
            bar: bar.upcast(),
            mute,
            camera,
            shown_remote: None,
            shown_local: None,
            shown_chat: None,
        }
    }

    /// Shows the call; `false` when there is none to show.
    pub fn sync(&mut self, app: &App) -> bool {
        let Some(call) = app.call.as_ref().filter(|_| !app.call_surface_hidden) else {
            self.shown_chat = None;
            return false;
        };
        let locale = app.locale;
        let title = app.chat_title_of(&call.chat);
        self.name.set_text(&title);
        if self.shown_chat.as_deref() != Some(call.chat.as_str()) {
            self.avatar.set_text(Some(&title));
            let picture = app
                .avatar_path(&call.chat)
                .as_deref()
                .and_then(super::pictures::file);
            self.avatar.set_custom_image(picture.as_ref());
            self.shown_chat = Some(call.chat.clone());
        }
        self.status
            .set_text(&whatsawra::ui::call::status(locale, call));
        let incoming = call.phase == CallPhase::Incoming;
        let over = matches!(call.phase, CallPhase::Ended | CallPhase::Failed);
        self.ringing.set_visible(incoming);
        self.bar.set_visible(!incoming && !over);
        if self.mute.is_active() != call.muted {
            self.mute.set_active(call.muted);
        }
        if self.camera.is_active() != call.camera_on {
            self.camera.set_active(call.camera_on);
        }
        self.camera.set_visible(call.video);

        let remote = call
            .video
            .then(|| app.call_remote_frame.clone())
            .flatten()
            .filter(|_| call.remote_video);
        self.shown_remote = show_frame(&self.remote, remote.as_ref(), self.shown_remote, None);
        let local = call
            .camera_on
            .then(|| app.call_local_frame.clone())
            .flatten();
        self.shown_local = show_frame(
            &self.local,
            local.as_ref(),
            self.shown_local,
            Some(LOCAL_WIDTH),
        );
        // The avatar steps aside for the peer's picture.
        self.avatar.set_visible(self.shown_remote.is_none());
        if call.phase == CallPhase::Active {
            // The timer moves on its own.
            awra::glib::timeout_add_local_once(std::time::Duration::from_millis(500), super::wake);
        }
        true
    }
}

/// Puts a video frame in `image` when it is a new one; returns which frame
/// is shown (its address), `None` when there is none.
fn show_frame(
    image: &awra::Image,
    frame: Option<&Arc<egui::ColorImage>>,
    shown: Option<usize>,
    width_at_most: Option<usize>,
) -> Option<usize> {
    let Some(frame) = frame else {
        if shown.is_some() {
            image.set_texture(None);
        }
        image.set_visible(false);
        return None;
    };
    image.set_visible(true);
    let key = Arc::as_ptr(frame) as usize;
    if shown != Some(key) {
        let [width, height] = frame.size;
        let rgba: &[u8] = bytemuck_rgba(&frame.pixels);
        // An image is as large as its picture: a small one is made small.
        let step = width_at_most.map_or(1, |most| width.div_ceil(most.max(1)).max(1));
        let texture = if step == 1 {
            awra::Texture::from_rgba(width as i32, height as i32, rgba, width * 4)
        } else {
            let (w, h) = (width / step, height / step);
            let mut small = Vec::with_capacity(w * h * 4);
            for y in 0..h {
                for x in 0..w {
                    let at = ((y * step) * width + x * step) * 4;
                    small.extend_from_slice(&rgba[at..at + 4]);
                }
            }
            awra::Texture::from_rgba(w as i32, h as i32, &small, w * 4)
        };
        if let Some(texture) = texture {
            image.set_texture(Some(&texture));
        }
    }
    Some(key)
}

/// A frame's pixels as RGBA bytes (opaque video: premultiplied is straight).
fn bytemuck_rgba(pixels: &[egui::Color32]) -> &[u8] {
    // SAFETY: Color32 is four u8 in RGBA order, repr(C), with no padding.
    unsafe { std::slice::from_raw_parts(pixels.as_ptr().cast::<u8>(), pixels.len() * 4) }
}
