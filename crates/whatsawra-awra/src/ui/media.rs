//! What a message carries besides its text: a picture (photo, sticker,
//! video's first frame), a voice message or a document, inside its bubble.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::{Rc, Weak};
use std::sync::Arc;

use awra::prelude::*;

use super::{Driver, Op, pictures};
use whatsawra::app::App;
use whatsawra::audio::State;
use whatsawra::model::{Action, Content, MediaState, Message};

/// The widest a picture is drawn in a bubble, and the tallest.
const PICTURE_WIDTH: f64 = 300.0;
const PICTURE_HEIGHT: f64 = 340.0;
const STICKER_SIDE: f64 = 150.0;
/// A picture or video with nothing to show yet.
const EMPTY_WIDTH: i32 = 220;
const EMPTY_HEIGHT: i32 = 124;

#[derive(Clone, Debug, PartialEq)]
pub enum MediaView {
    Picture {
        path: Option<PathBuf>,
        /// The decoded photo is ready to show (else the thumbnail).
        ready: bool,
        thumbnail: bool,
        width: i32,
        height: i32,
        sticker: bool,
        /// A video's length, shown over its first frame.
        video: Option<String>,
        loading: bool,
        failed: Option<String>,
        /// The clip playing in the bubble, when it is this one.
        playing: Option<Playing>,
    },
    Voice {
        path: Option<PathBuf>,
        peaks: Vec<f32>,
        seconds: f64,
        position: Option<f64>,
        playing: bool,
        loading: bool,
    },
    Document {
        path: Option<PathBuf>,
        name: String,
        detail: String,
        loading: bool,
    },
    Poll(Box<super::polls::PollView>),
}

/// A clip playing in its bubble: its frame and where it is.
#[derive(Clone, Debug)]
pub struct Playing {
    pub paused: bool,
    pub muted: bool,
    /// "0:12 / 0:40".
    pub time: String,
    pub frame: Option<Arc<egui::ColorImage>>,
}

impl PartialEq for Playing {
    fn eq(&self, other: &Self) -> bool {
        self.paused == other.paused
            && self.muted == other.muted
            && self.time == other.time
            && match (&self.frame, &other.frame) {
                (Some(a), Some(b)) => Arc::ptr_eq(a, b),
                (None, None) => true,
                _ => false,
            }
    }
}

fn playing(app: &App, message: &str) -> Option<Playing> {
    let status = app.video.status(message)?;
    // Drawn in the chat: a clip left out of sight long enough pauses.
    app.video.saw(message);
    let at = status.position.as_secs() as u32;
    Some(Playing {
        paused: status.state == whatsawra::video::State::Paused,
        muted: app.video.muted(),
        time: if status.total.is_zero() {
            whatsawra::util::duration(at)
        } else {
            format!(
                "{} / {}",
                whatsawra::util::duration(at),
                whatsawra::util::duration(status.total.as_secs() as u32)
            )
        },
        frame: status.image,
    })
}

/// What a message shows of its media, and whether to fetch it now.
pub fn view_of(app: &App, message: &Message) -> Option<(MediaView, bool)> {
    if let Some(poll) = super::polls::view(app, message) {
        return Some((MediaView::Poll(Box::new(poll)), false));
    }
    let media = message.content.media()?;
    let loading = matches!(media.state, MediaState::Downloading);
    let failed = match &media.state {
        MediaState::Failed(error) => Some(error.clone()),
        _ => None,
    };
    let auto = media.path.is_none()
        && matches!(media.state, MediaState::Idle)
        && app.account().settings.auto_download
        && media.is_within_download_limit();
    let picture = |sticker: bool, video: Option<String>| {
        let playing = video.as_ref().and_then(|_| playing(app, &message.id));
        // Nothing to show yet (no file, no thumbnail): a small frame, not
        // a picture-sized hole.
        let (width, height) = if media.path.is_none() && message.thumbnail.is_none() && !sticker {
            (EMPTY_WIDTH, EMPTY_HEIGHT)
        } else {
            fit(
                media.width.unwrap_or(0) as f64,
                media.height.unwrap_or(0) as f64,
                sticker,
            )
        };
        MediaView::Picture {
            ready: media.path.as_deref().is_some_and(pictures::photo_ready),
            path: media.path.clone(),
            thumbnail: message.thumbnail.is_some(),
            width,
            height,
            sticker,
            video,
            loading,
            failed: failed.clone(),
            playing,
        }
    };
    let view = match &message.content {
        Content::Image { .. } => picture(false, None),
        Content::Sticker { .. } => picture(true, None),
        Content::Video { seconds, .. } => picture(
            false,
            Some(seconds.map(whatsawra::util::duration).unwrap_or_default()),
        ),
        Content::Audio {
            seconds, waveform, ..
        } => {
            let status = app.player.status(&message.id);
            let total = if status.total.is_zero() {
                f64::from(seconds.unwrap_or(0))
            } else {
                status.total.as_secs_f64()
            };
            let top = f32::from(waveform.iter().copied().max().unwrap_or(0).max(1));
            MediaView::Voice {
                path: media.path.clone(),
                peaks: waveform.iter().map(|bar| f32::from(*bar) / top).collect(),
                seconds: total,
                position: (!matches!(status.state, State::Idle))
                    .then_some(status.position.as_secs_f64()),
                playing: matches!(status.state, State::Playing | State::Loading),
                loading,
            }
        }
        Content::Document {
            file_name, pages, ..
        } => {
            let mut detail = whatsawra::util::bytes(media.size);
            if let Some(pages) = pages.filter(|pages| *pages > 0) {
                detail = format!("{pages} pages · {detail}");
            }
            MediaView::Document {
                path: media.path.clone(),
                name: file_name.clone(),
                detail,
                loading,
            }
        }
        _ => return None,
    };
    Some((view, auto))
}

/// A picture's size in the bubble, keeping its shape.
fn fit(width: f64, height: f64, sticker: bool) -> (i32, i32) {
    if sticker {
        return (STICKER_SIDE as i32, STICKER_SIDE as i32);
    }
    if width <= 0.0 || height <= 0.0 {
        return (PICTURE_WIDTH as i32, (PICTURE_WIDTH * 0.75) as i32);
    }
    let scale = (PICTURE_WIDTH / width)
        .min(PICTURE_HEIGHT / height)
        .min(1.0);
    let (w, h) = (width * scale, height * scale);
    // Very narrow pictures keep a usable width.
    ((w.max(120.0)) as i32, (h.max(80.0)) as i32)
}

/// The widgets of a message's media, made once per row.
#[derive(Clone)]
pub struct MediaWidgets {
    pub root: awra::Box,
    picture: awra::Image,
    picture_badge: awra::Label,
    sound: awra::Button,
    /// The frame on screen, by address, while a clip plays.
    frame: Rc<std::cell::Cell<Option<usize>>>,
    voice: awra::Box,
    play: awra::Button,
    waveform: awra::WaveformView,
    voice_time: awra::Label,
    document: awra::Box,
    document_name: awra::Label,
    document_detail: awra::Label,
    poll: super::polls::PollWidgets,
    shown: Rc<RefCell<Option<(String, MediaView)>>>,
}

impl MediaWidgets {
    pub fn build(driver: &Weak<Driver>) -> Self {
        let root = awra::Box::new(awra::Orientation::Vertical, 0);
        let shown: Rc<RefCell<Option<(String, MediaView)>>> = Rc::default();

        let overlay = awra::Overlay::new();
        let picture = awra::Image::new();
        picture.set_corner_radius(10.0);
        picture.set_content_fit(awra::ContentFit::Cover);
        overlay.set_child(Some(&picture));
        let picture_badge = awra::Label::new(None);
        picture_badge.set_halign(awra::Align::Start);
        picture_badge.set_valign(awra::Align::End);
        picture_badge.set_margin_start(8);
        picture_badge.set_margin_bottom(8);
        picture_badge.set_weight(600);
        overlay.add_overlay(&picture_badge);
        // A playing clip's sound, in its top corner.
        let sound = awra::Button::from_icon_name("audio-volume-high-symbolic");
        sound.set_appearance(awra::ButtonAppearance::Secondary);
        sound.set_halign(awra::Align::End);
        sound.set_valign(awra::Align::Start);
        sound.set_margin_top(6);
        sound.set_margin_end(6);
        sound.set_visible(false);
        let weak = driver.clone();
        sound.connect_clicked(move |_| {
            if let Some(driver) = weak.upgrade() {
                driver.act(Action::ToggleVideoSound);
            }
        });
        overlay.add_overlay(&sound);
        root.append(&overlay);
        click(&picture, driver, &shown);

        let voice = awra::Box::new(awra::Orientation::Horizontal, 8);
        let play = awra::Button::from_icon_name("media-playback-start-symbolic");
        play.set_appearance(awra::ButtonAppearance::Ghost);
        play.set_valign(awra::Align::Center);
        let waveform = awra::WaveformView::new();
        waveform.set_size_request(200, 36);
        waveform.set_hexpand(true);
        let voice_time = awra::Label::new(None);
        voice_time.set_typography(awra::Typography::Muted);
        voice_time.set_tabular_digits(true);
        voice.append(&play);
        voice.append(&waveform);
        voice.append(&voice_time);
        root.append(&voice);
        let (weak, playing) = (driver.clone(), shown.clone());
        play.connect_clicked(move |_| {
            let Some(driver) = weak.upgrade() else { return };
            if let Some((id, MediaView::Voice { path, .. })) = playing.borrow().clone() {
                driver.act(match path {
                    Some(path) => Action::PlayVoice { message: id, path },
                    None => download(&driver, id),
                });
            }
        });
        let (weak, seeking) = (driver.clone(), shown.clone());
        waveform.connect_seek_requested(move |_, seconds| {
            let Some(driver) = weak.upgrade() else { return };
            if let Some((
                id,
                MediaView::Voice {
                    path: Some(path),
                    seconds: total,
                    ..
                },
            )) = seeking.borrow().clone()
                && total > 0.0
            {
                driver.act(Action::SeekVoice {
                    message: id,
                    path,
                    fraction: (seconds / total).clamp(0.0, 1.0) as f32,
                });
            }
        });

        let document = awra::Box::new(awra::Orientation::Horizontal, 10);
        let icon = awra::Image::from_icon_name("x-office-document-symbolic");
        icon.set_pixel_size(28);
        let lines = awra::Box::new(awra::Orientation::Vertical, 0);
        let document_name = awra::Label::new(None);
        document_name.set_xalign(0.0);
        document_name.set_weight(600);
        document_name.set_ellipsize(awra::pango::EllipsizeMode::Middle);
        document_name.set_max_width_chars(36);
        let document_detail = awra::Label::new(None);
        document_detail.set_xalign(0.0);
        document_detail.set_typography(awra::Typography::Muted);
        lines.append(&document_name);
        lines.append(&document_detail);
        document.append(&icon);
        document.append(&lines);
        document.set_margin_bottom(2);
        root.append(&document);
        click(&document, driver, &shown);
        let locale = driver
            .upgrade()
            .map_or(whatsawra::i18n::Locale::English, |driver| {
                driver.read(|app| app.locale)
            });
        let poll = super::polls::PollWidgets::build(driver, locale);
        root.append(&poll.root);

        Self {
            root,
            picture,
            picture_badge,
            sound,
            frame: Rc::default(),
            voice,
            play,
            waveform,
            voice_time,
            document,
            document_name,
            document_detail,
            poll,
            shown,
        }
    }

    pub fn bind(&self, message: &str, media: Option<&MediaView>, thumbnail: Option<&[u8]>) {
        let Some(media) = media else {
            self.root.set_visible(false);
            *self.shown.borrow_mut() = None;
            return;
        };
        self.root.set_visible(true);
        let is = |kind: u8| match media {
            MediaView::Picture { .. } => kind == 0,
            MediaView::Voice { .. } => kind == 1,
            MediaView::Document { .. } => kind == 2,
            MediaView::Poll(_) => kind == 3,
        };
        self.picture
            .parent()
            .inspect(|overlay| overlay.set_visible(is(0)));
        self.voice.set_visible(is(1));
        self.document.set_visible(is(2));
        match media {
            MediaView::Poll(poll) => self.poll.bind(message, Some(poll)),
            _ => self.poll.bind(message, None),
        }
        match media {
            MediaView::Picture {
                path,
                width,
                height,
                sticker,
                video,
                loading,
                failed,
                playing,
                ..
            } => {
                self.picture.set_size_request(*width, *height);
                self.picture.set_content_fit(if *sticker {
                    awra::ContentFit::Contain
                } else {
                    awra::ContentFit::Cover
                });
                let texture = path
                    .as_deref()
                    .and_then(pictures::photo)
                    .or_else(|| thumbnail.and_then(|jpeg| pictures::thumbnail(message, jpeg)));
                match playing.as_ref().and_then(|playing| playing.frame.as_ref()) {
                    // The clip's own frame, once one is decoded.
                    Some(frame) => {
                        let shown = self.frame.get();
                        self.frame.set(super::call::show_frame(
                            &self.picture,
                            Some(frame),
                            shown,
                            None,
                        ));
                    }
                    None => {
                        self.frame.set(None);
                        self.picture.set_visible(true);
                        self.picture.set_texture(texture.as_ref());
                    }
                }
                self.sound.set_visible(playing.is_some());
                if let Some(playing) = playing {
                    self.sound.set_icon_name(Some(if playing.muted {
                        "audio-volume-muted-symbolic"
                    } else {
                        "audio-volume-high-symbolic"
                    }));
                }
                let badge = if let Some(playing) = playing {
                    let sign = if playing.paused { "▶" } else { "⏸" };
                    format!("{sign} {}", playing.time)
                } else if let Some(error) = failed {
                    format!("⚠ {error}")
                } else if *loading {
                    "⭳ …".to_owned()
                } else if path.is_none() && !sticker {
                    "⭳".to_owned()
                } else if let Some(length) = video {
                    format!("▶ {length}")
                } else {
                    String::new()
                };
                self.picture_badge.set_visible(!badge.is_empty());
                self.picture_badge.set_text(&badge);
            }
            MediaView::Voice {
                peaks,
                seconds,
                position,
                playing,
                loading,
                ..
            } => {
                self.waveform.set_peaks(peaks);
                self.waveform.set_duration(*seconds);
                self.waveform.set_position(position.unwrap_or(-1.0));
                self.play.set_icon_name(Some(if *playing {
                    "media-playback-pause-symbolic"
                } else {
                    "media-playback-start-symbolic"
                }));
                self.play.set_busy(*loading);
                let shown = position.unwrap_or(*seconds).max(0.0) as u32;
                self.voice_time.set_text(&whatsawra::util::duration(shown));
            }
            MediaView::Poll(_) => {}
            MediaView::Document {
                name,
                detail,
                loading,
                ..
            } => {
                self.document_name.set_text(name);
                self.document_detail.set_text(&if *loading {
                    format!("{detail} · …")
                } else {
                    detail.clone()
                });
            }
        }
        *self.shown.borrow_mut() = Some((message.to_owned(), media.clone()));
    }
}

fn download(driver: &Rc<Driver>, message: String) -> Action {
    Action::Download {
        manual: true,
        card: None,
        chat: driver.read(|app| app.open_chat.clone()).unwrap_or_default(),
        message,
    }
}

/// A click on a picture or a document: opens what is downloaded, fetches
/// what is not.
fn click(
    widget: &impl awra::IsWidget,
    driver: &Weak<Driver>,
    shown: &Rc<RefCell<Option<(String, MediaView)>>>,
) {
    let gesture = awra::GestureClick::new();
    gesture.set_button(1);
    let (weak, shown) = (driver.clone(), shown.clone());
    gesture.connect_released(move |_, _, _, _| {
        let Some(driver) = weak.upgrade() else { return };
        let Some((id, media)) = shown.borrow().clone() else {
            return;
        };
        let (path, loading, video, picture) = match media {
            MediaView::Picture {
                path,
                loading,
                video,
                ..
            } => (path, loading, video.is_some(), true),
            MediaView::Document { path, loading, .. } => (path, loading, false, false),
            MediaView::Voice { .. } | MediaView::Poll(_) => return,
        };
        match path {
            // A clip plays in its bubble; a picture opens in the viewer over
            // the chat's album; anything else goes to the system's own app.
            Some(path) if video => driver.act(Action::PlayVideo { message: id, path }),
            Some(path)
                if picture
                    && whatsawra::image_preview::open_target(&path, true)
                        == whatsawra::image_preview::OpenTarget::Preview =>
            {
                let chat = driver.read(|app| app.open_chat.clone()).unwrap_or_default();
                driver.act(Action::PreviewImage {
                    path,
                    chat,
                    message: id,
                    card: None,
                });
            }
            Some(path) => driver.act(Action::OpenFile(path)),
            None if !loading => {
                let action = download(&driver, id);
                driver.act(action);
            }
            None => {}
        }
    });
    widget.upcast_ref().add_controller(gesture);
}

/// Asks once for each message's media the reader is about to see.
pub fn fetch(
    driver: &Rc<Driver>,
    asked: &RefCell<std::collections::HashSet<String>>,
    chat: &str,
    message: &str,
) {
    if asked.borrow_mut().insert(message.to_owned()) {
        driver.later(Op::Action(Action::Download {
            manual: false,
            card: None,
            chat: chat.to_owned(),
            message: message.to_owned(),
        }));
    }
}
