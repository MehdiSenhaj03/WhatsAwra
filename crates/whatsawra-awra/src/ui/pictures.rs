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

#[derive(Default)]
struct Cache {
    /// Small files read at once (profile pictures).
    files: HashMap<PathBuf, Option<awra::Texture>>,
    /// Photos decoded off the UI thread, by path.
    /// Decoded pictures by path and the longest side they were scaled to.
    photos: HashMap<(PathBuf, u32), Option<awra::Texture>>,
    loading: HashSet<(PathBuf, u32)>,
    /// Thumbnails sent with messages, by message id.
    thumbnails: HashMap<String, Option<awra::Texture>>,
}

thread_local! {
    static CACHE: RefCell<Cache> = RefCell::new(Cache::default());
}

/// A small picture file (a profile picture), read now.
pub fn file(path: &Path) -> Option<awra::Texture> {
    CACHE.with(|cache| {
        cache
            .borrow_mut()
            .files
            .entry(path.to_path_buf())
            .or_insert_with(|| awra::Texture::from_filename(path).ok())
            .clone()
    })
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
    CACHE.with(|cache| {
        cache
            .borrow()
            .photos
            .contains_key(&(path.to_path_buf(), PHOTO_SIDE))
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
            let path = path.to_path_buf();
            std::thread::Builder::new()
                .name("awra-picture".into())
                .spawn(move || {
                    let decoded = decode(&path, side);
                    awra::glib::invoke(move || {
                        let texture = decoded.and_then(|(rgba, width, height)| {
                            awra::Texture::from_rgba(
                                width as i32,
                                height as i32,
                                &rgba,
                                width as usize * 4,
                            )
                        });
                        CACHE.with(|cache| {
                            let mut cache = cache.borrow_mut();
                            cache.loading.remove(&(path.clone(), side));
                            cache.photos.insert((path, side), texture);
                        });
                        super::wake();
                    });
                })
                .ok();
        }
        None
    })
}

/// Reads and scales a picture to straight-alpha RGBA.
fn decode(path: &Path, side: u32) -> Option<(Vec<u8>, u32, u32)> {
    let image = image::ImageReader::open(path)
        .ok()?
        .with_guessed_format()
        .ok()?
        .decode()
        .ok()?;
    let image = if image.width().max(image.height()) > side {
        image.thumbnail(side, side)
    } else {
        image
    };
    let rgba = image.into_rgba8();
    let (width, height) = rgba.dimensions();
    Some((rgba.into_raw(), width, height))
}
