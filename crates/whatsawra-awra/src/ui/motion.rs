//! GIFs that move: a GIF (WhatsApp sends them as short clips) plays in its
//! bubble while the pointer is over it, as in the egui window, and rests
//! on its poster otherwise. Frames decode once per file, off this thread.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

/// The longest side frames are kept at: a bubble's picture, twice over.
const SIDE: u32 = 600;
/// How often a playing GIF looks for its next frame.
const TICK: Duration = Duration::from_millis(30);

type Frames = Rc<Vec<(awra::Texture, Duration)>>;
/// A decoded frame on its way to this thread: RGBA, its size, how long it shows.
type Pixels = (Vec<u8>, u32, u32, Duration);

thread_local! {
    static DECODED: RefCell<HashMap<PathBuf, Option<Frames>>> = RefCell::default();
    static LOADING: RefCell<HashSet<PathBuf>> = RefCell::default();
}

/// The frames of `path` once decoded; asks for them otherwise.
fn frames(path: &Path) -> Option<Frames> {
    if let Some(known) = DECODED.with(|decoded| decoded.borrow().get(path).cloned()) {
        return known;
    }
    if LOADING.with(|loading| loading.borrow_mut().insert(path.to_path_buf())) {
        let path = path.to_path_buf();
        std::thread::Builder::new()
            .name("awra-gif".into())
            .spawn(move || {
                let frames: Option<Vec<Pixels>> =
                    whatsawra::animation::decode_frames(&path, usize::MAX).map(|frames| {
                        frames
                            .iter()
                            .filter_map(|(frame, delay)| {
                                let mut rgba = super::pictures::color_image(frame)?;
                                if rgba.width().max(rgba.height()) > SIDE {
                                    rgba = image::imageops::thumbnail(
                                        &rgba,
                                        rgba.width() * SIDE / rgba.width().max(rgba.height()),
                                        rgba.height() * SIDE / rgba.width().max(rgba.height()),
                                    );
                                }
                                let (width, height) = rgba.dimensions();
                                Some((rgba.into_raw(), width, height, *delay))
                            })
                            .collect()
                    });
                awra::glib::invoke(move || {
                    let textures = frames.and_then(|frames| {
                        let textures: Vec<_> = frames
                            .into_iter()
                            .filter_map(|(rgba, width, height, delay)| {
                                let texture = awra::Texture::from_rgba(
                                    width as i32,
                                    height as i32,
                                    &rgba,
                                    width as usize * 4,
                                )?;
                                Some((texture, delay.max(Duration::from_millis(20))))
                            })
                            .collect();
                        (!textures.is_empty()).then(|| Rc::new(textures))
                    });
                    LOADING.with(|loading| loading.borrow_mut().remove(&path));
                    DECODED.with(|decoded| decoded.borrow_mut().insert(path, textures));
                });
            })
            .ok();
    }
    None
}

/// Plays `path` in `image` until the returned flag is set.
pub fn play(image: &awra::Image, path: PathBuf) -> Rc<Cell<bool>> {
    let stop = Rc::new(Cell::new(false));
    let (image, stopped) = (image.clone(), stop.clone());
    let started = Instant::now();
    let shown = Cell::new(usize::MAX);
    awra::glib::timeout_add_local(TICK, move || {
        if stopped.get() {
            return awra::glib::ControlFlow::Break;
        }
        let Some(frames) = frames(&path) else {
            return awra::glib::ControlFlow::Continue;
        };
        let total: Duration = frames.iter().map(|(_, delay)| *delay).sum();
        let mut at =
            Duration::from_nanos((started.elapsed().as_nanos() % total.as_nanos().max(1)) as u64);
        let index = frames
            .iter()
            .position(|(_, delay)| {
                if at < *delay {
                    true
                } else {
                    at -= *delay;
                    false
                }
            })
            .unwrap_or(0);
        if shown.replace(index) != index {
            image.set_texture(Some(&frames[index].0));
        }
        awra::glib::ControlFlow::Continue
    });
    stop
}
