//! Pictures for the views: profile pictures, message thumbnails, photos and
//! stickers, kept as Awra textures on the UI thread. A photo is decoded and
//! scaled down on a thread of its own; until it is ready the view shows the
//! message's thumbnail, and the next turn picks it up.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// The longest side a photo is kept at: about twice a bubble's, for sharp
/// pictures on a high-density screen.
pub const PHOTO_SIDE: u32 = 720;
/// The longest side a picture is kept at in the viewer.
pub const FULL_SIDE: u32 = 2560;
/// The longest side of a picker tile's picture.
pub const TILE_SIDE: u32 = 192;
/// The longest side a profile picture is kept at: the largest avatar (the
/// info panel's) on a high-density screen.
pub const AVATAR_SIDE: u32 = 256;
/// How many avatars are remembered as showing a picture.
const SHOWING_MOST: usize = 512;

#[derive(Default)]
struct Cache {
    /// Photos decoded off the UI thread, by path.
    /// Decoded pictures by path and the longest side they were scaled to.
    photos: HashMap<(PathBuf, u32), Option<awra::Texture>>,
    loading: HashSet<(PathBuf, u32)>,
    /// Thumbnails sent with messages, by message id.
    thumbnails: HashMap<String, Option<awra::Texture>>,
}

thread_local! {
    static CACHE: RefCell<Cache> = RefCell::new(Cache::default());
    /// Avatars whose picture is being decoded.
    static WAITING: RefCell<Vec<(PathBuf, awra::Avatar)>> = const { RefCell::new(Vec::new()) };
    /// The avatars showing a picture, to give them the new one when its file
    /// changes; the oldest are let go past [`SHOWING_MOST`].
    static SHOWING: RefCell<Vec<(PathBuf, awra::Avatar)>> = const { RefCell::new(Vec::new()) };
}

/// Puts the profile picture at `path` in `avatar` (its initials without
/// one). It decodes off this thread the first time, so a list of faces never
/// holds the window up; the avatar fills in once it is ready, unless it has
/// been given another picture by then.
pub fn avatar(avatar: &awra::Avatar, path: Option<&Path>) {
    let wanted = path.map(Path::to_path_buf);
    if avatar.rust_data::<Option<PathBuf>>("picture").as_ref() == Some(&wanted) {
        return;
    }
    avatar.set_rust_data("picture", wanted.clone());
    SHOWING.with(|showing| {
        let mut showing = showing.borrow_mut();
        showing.retain(|(_, shown)| shown != avatar);
        if let Some(path) = &wanted {
            if showing.len() >= SHOWING_MOST {
                showing.remove(0);
            }
            showing.push((path.clone(), avatar.clone()));
        }
    });
    let Some(path) = wanted else {
        avatar.set_custom_image(None::<&awra::Texture>);
        return;
    };
    if let Some(texture) = load(&path, AVATAR_SIDE) {
        avatar.set_custom_image(Some(&texture));
        return;
    }
    avatar.set_custom_image(None::<&awra::Texture>);
    if photo_ready_at(&path, AVATAR_SIDE) {
        return;
    }
    WAITING.with(|waiting| waiting.borrow_mut().push((path, avatar.clone())));
}

/// Forgets the pictures whose file changed (`uris` as `util::image_uri`
/// writes them: a new profile picture keeps the old one's file name) and
/// gives the avatars showing them the new one.
pub fn forget(uris: &[String]) {
    if uris.is_empty() {
        return;
    }
    let changed: Vec<PathBuf> = CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        let changed: Vec<PathBuf> = cache
            .photos
            .keys()
            .map(|(path, _)| path)
            .filter(|path| uris.contains(&whatsawra::util::image_uri(path)))
            .cloned()
            .collect();
        cache.photos.retain(|(path, _), _| !changed.contains(path));
        changed
    });
    if changed.is_empty() {
        return;
    }
    let showing: Vec<(PathBuf, awra::Avatar)> = SHOWING.with(|showing| {
        showing
            .borrow()
            .iter()
            .filter(|(path, _)| changed.contains(path))
            .cloned()
            .collect()
    });
    for (path, shown) in showing {
        // Not the picture it has: `avatar` reads the file again.
        shown.set_rust_data("picture", None::<PathBuf>);
        avatar(&shown, Some(&path));
    }
}

/// Fills the avatars that waited for `path`, once it is decoded.
fn fill_waiting(path: &Path) {
    let ready: Vec<awra::Avatar> = WAITING.with(|waiting| {
        let mut waiting = waiting.borrow_mut();
        let (ready, rest) = std::mem::take(&mut *waiting)
            .into_iter()
            .partition(|(waited, _)| waited == path);
        *waiting = rest;
        ready.into_iter().map(|(_, avatar)| avatar).collect()
    });
    if ready.is_empty() {
        return;
    }
    let texture = load(path, AVATAR_SIDE);
    for avatar in ready {
        // Given another picture meanwhile: that one wins.
        if avatar
            .rust_data::<Option<PathBuf>>("picture")
            .flatten()
            .as_deref()
            == Some(path)
        {
            avatar.set_custom_image(texture.as_ref());
        }
    }
}

/// A message's embedded JPEG thumbnail.
pub fn thumbnail(message: &str, jpeg: &[u8]) -> Option<awra::Texture> {
    CACHE.with(|cache| {
        cache
            .borrow_mut()
            .thumbnails
            .entry(message.to_owned())
            .or_insert_with(|| awra::Texture::from_bytes(jpeg).ok())
            .clone()
    })
}

/// Whether the photo at `path` is decoded (or failed to).
pub fn photo_ready(path: &Path) -> bool {
    photo_ready_at(path, PHOTO_SIDE)
}

/// Whether the picture at `path` is decoded at `side` (or failed to).
fn photo_ready_at(path: &Path, side: u32) -> bool {
    CACHE.with(|cache| {
        cache
            .borrow()
            .photos
            .contains_key(&(path.to_path_buf(), side))
    })
}

/// The photo at `path`, scaled to [`PHOTO_SIDE`] at most; `None` while it
/// is being decoded (it starts now), or when it cannot be.
pub fn photo(path: &Path) -> Option<awra::Texture> {
    load(path, PHOTO_SIDE)
}

/// A tile's picture (a sticker, a GIF's still), up to [`TILE_SIDE`].
pub fn tile(path: &Path) -> Option<awra::Texture> {
    load(path, TILE_SIDE)
}

/// The picture at `path` for the viewer, up to [`FULL_SIDE`].
pub fn full(path: &Path) -> Option<awra::Texture> {
    load(path, FULL_SIDE)
}

fn load(path: &Path, side: u32) -> Option<awra::Texture> {
    let key = (path.to_path_buf(), side);
    CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if let Some(texture) = cache.photos.get(&key) {
            return texture.clone();
        }
        if cache.loading.insert(key.clone()) {
            decoders().ask(key);
        }
        None
    })
}

/// The threads that decode pictures. A chat full of photos and clips once
/// started a thread for each: twenty decodes at a time took every core, and
/// the window stuttered while they ran. A few decode now, the picture asked
/// for last first, since it is the one on screen (the chat just opened, the
/// rows just scrolled to).
struct Decoders {
    wanted: std::sync::Mutex<Vec<(PathBuf, u32)>>,
    more: std::sync::Condvar,
}

/// How many pictures decode at once.
const DECODERS: usize = 3;

impl Decoders {
    fn ask(&self, key: (PathBuf, u32)) {
        if let Ok(mut wanted) = self.wanted.lock() {
            wanted.push(key);
        }
        self.more.notify_one();
    }

    fn work(&self) {
        loop {
            let (path, side) = {
                let Ok(mut wanted) = self.wanted.lock() else {
                    return;
                };
                loop {
                    if let Some(key) = wanted.pop() {
                        break key;
                    }
                    wanted = match self.more.wait(wanted) {
                        Ok(wanted) => wanted,
                        Err(_) => return,
                    };
                }
            };
            let decoded = decode(&path, side);
            awra::glib::invoke(move || {
                let texture = decoded.and_then(|(rgba, width, height)| {
                    awra::Texture::from_rgba(width as i32, height as i32, &rgba, width as usize * 4)
                });
                CACHE.with(|cache| {
                    let mut cache = cache.borrow_mut();
                    cache.loading.remove(&(path.clone(), side));
                    cache.photos.insert((path.clone(), side), texture);
                });
                if side == AVATAR_SIDE {
                    fill_waiting(&path);
                }
                super::wake();
            });
        }
    }
}

fn decoders() -> &'static Decoders {
    static DECODING: std::sync::OnceLock<&'static Decoders> = std::sync::OnceLock::new();
    DECODING.get_or_init(|| {
        let decoders: &'static Decoders = Box::leak(Box::new(Decoders {
            wanted: std::sync::Mutex::new(Vec::new()),
            more: std::sync::Condvar::new(),
        }));
        for _ in 0..DECODERS {
            let _ = std::thread::Builder::new()
                .name("awra-picture".into())
                .spawn(move || decoders.work());
        }
        decoders
    })
}

/// A clip's picture (a video, a GIF sent as one): a frame about a second
/// in, since openings are often black, or its last when it is shorter.
fn poster(path: &Path) -> Option<image::DynamicImage> {
    let (frame, _) = whatsawra::animation::decode_frames(path, 25)?.pop()?;
    color_image(&frame).map(image::DynamicImage::ImageRgba8)
}

/// An egui picture as straight-alpha RGBA.
pub fn color_image(frame: &egui::ColorImage) -> Option<image::RgbaImage> {
    let [width, height] = frame.size;
    let rgba = frame
        .pixels
        .iter()
        .flat_map(|pixel| pixel.to_srgba_unmultiplied())
        .collect();
    image::RgbaImage::from_raw(width as u32, height as u32, rgba)
}

/// Reads and scales a picture to straight-alpha RGBA.
fn decode(path: &Path, side: u32) -> Option<(Vec<u8>, u32, u32)> {
    let image = image::ImageReader::open(path)
        .ok()
        .and_then(|reader| reader.with_guessed_format().ok())
        .and_then(|reader| reader.decode().ok())
        .or_else(|| poster(path))?;
    let image = if image.width().max(image.height()) > side {
        image.thumbnail(side, side)
    } else {
        image
    };
    let rgba = image.into_rgba8();
    let (width, height) = rgba.dimensions();
    Some((rgba.into_raw(), width, height))
}

#[cfg(all(test, feature = "demo"))]
mod tests {
    #[test]
    fn a_clip_has_a_poster() {
        let dir = std::env::temp_dir().join("whatsawra-awra-poster-test");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("clip.mp4");
        std::fs::write(&path, whatsawra::demo::stock::VIDEO).unwrap();
        let started = std::time::Instant::now();
        let poster = super::decode(&path, super::PHOTO_SIDE);
        eprintln!("poster in {:?}", started.elapsed());
        assert!(poster.is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
