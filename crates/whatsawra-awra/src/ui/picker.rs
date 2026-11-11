//! The composer's picker: emoji, GIFs and stickers behind three tabs in one
//! popover, as the egui picker has them. GIFs and stickers are the app's
//! (`App::gif_results`, the account's stickers); a pick sends at once.

use std::path::PathBuf;
use std::rc::Rc;

use awra::prelude::*;

use super::{Driver, pictures};
use whatsawra::app::App;
use whatsawra::i18n::{Locale, gettext};
use whatsawra::model::{Action, Gif, PickerTab};

/// A tile's side in the grids.
const TILE: i32 = 76;
/// The pages' width.
const WIDTH: i32 = 364;
const TABS: [(PickerTab, &str); 3] = [
    (PickerTab::Emoji, "emoji"),
    (PickerTab::Gifs, "gifs"),
    (PickerTab::Stickers, "stickers"),
];

/// A picture in a grid, filled once its file is decoded.
struct Tile {
    path: Option<PathBuf>,
    image: awra::Image,
    filled: bool,
}

impl Tile {
    fn fill(&mut self) {
        if self.filled {
            return;
        }
        if let Some(texture) = self.path.as_deref().and_then(pictures::tile) {
            self.image.set_texture(Some(&texture));
            self.filled = true;
        }
    }
}

/// A button holding a picture, which runs `on_click`.
fn tile(
    path: Option<PathBuf>,
    fit: awra::ContentFit,
    on_click: impl Fn() + 'static,
) -> (awra::Button, Tile) {
    let image = awra::Image::new();
    image.set_size_request(TILE, TILE);
    image.set_pixel_size(TILE);
    image.set_content_fit(fit);
    image.set_corner_radius(8.0);
    let button = awra::Button::new();
    button.set_appearance(awra::ButtonAppearance::Ghost);
    button.set_child(Some(&image));
    button.connect_clicked(move |_| on_click());
    let mut tile = Tile {
        path,
        image,
        filled: false,
    };
    tile.fill();
    (button, tile)
}

fn grid() -> awra::TileGrid {
    let grid = awra::TileGrid::new();
    grid.set_columns(3, 4);
    grid.set_spacing(4, 4);
    grid
}

fn muted(text: &str) -> awra::Label {
    let label = awra::Label::new(Some(text));
    label.set_typography(awra::Typography::Muted);
    label.set_wrap(true);
    // Wrapped, it would otherwise ask for its whole sentence's width.
    label.set_max_width_chars(40);
    label.set_xalign(0.0);
    label
}

pub struct Picker {
    pub popover: awra::Popover,
    tabs: awra::SegmentedControl,
    stack: awra::Stack,
    locale: Locale,
    driver: std::rc::Weak<Driver>,
    // GIFs.
    gif_key: awra::Box,
    gif_key_entry: awra::Entry,
    gif_search: awra::SearchEntry,
    gif_status: awra::Label,
    gif_grid: awra::TileGrid,
    gif_more: awra::Button,
    gif_tiles: Vec<Tile>,
    gif_shown: Vec<(String, Option<PathBuf>)>,
    // Stickers.
    sticker_list: awra::Box,
    sticker_tiles: Vec<Tile>,
    sticker_shown: Option<(bool, Vec<(String, Vec<PathBuf>)>)>,
}

impl Picker {
    pub fn build(
        driver: &Rc<Driver>,
        locale: Locale,
        anchor: &awra::Button,
        composer: &awra::Composer,
    ) -> Self {
        let t = |text: &'static str| gettext(locale, text).into_owned();
        let popover = awra::Popover::new();
        popover.set_parent(anchor);
        popover.set_position(awra::PositionType::Top);

        let root = awra::Box::new(awra::Orientation::Vertical, 8);
        root.set_size_request(WIDTH, 420);
        root.set_margin_top(8);
        root.set_margin_bottom(8);
        root.set_margin_start(8);
        root.set_margin_end(8);
        let tabs = awra::SegmentedControl::new();
        tabs.append(&t("Emoji"));
        tabs.append(&t("GIFs"));
        tabs.append(&t("Stickers"));
        tabs.set_halign(awra::Align::Center);
        root.append(&tabs);
        let stack = awra::Stack::new();
        stack.set_vexpand(true);
        root.append(&stack);
        popover.set_child(Some(&root));

        let shown = popover.clone();
        let emoji = super::emoji::picker(composer, move || shown.popdown());
        stack.add_named(&emoji, Some("emoji"));

        // GIFs: a search, a grid of stills, more on request.
        let gifs = awra::Box::new(awra::Orientation::Vertical, 8);
        let gif_key = awra::Box::new(awra::Orientation::Vertical, 6);
        gif_key.append(&muted(&t(
            "GIF search needs a GIPHY API key. Create a free key at developers.giphy.com and paste it here. It is saved in your settings.",
        )));
        let gif_key_entry = awra::Entry::new();
        gif_key_entry.set_placeholder_text(t("GIPHY API key"));
        let weak = Rc::downgrade(driver);
        gif_key_entry.connect_changed(move |entry| {
            let key = entry.text();
            if let Some(driver) = weak.upgrade() {
                driver.edit(move |app| {
                    app.settings.giphy_key = key.trim().to_owned();
                    app.queue(Action::SettingsChanged);
                });
            }
        });
        gif_key.append(&gif_key_entry);
        let use_key = awra::Button::with_label(&t("Search GIFs"));
        use_key.set_halign(awra::Align::Start);
        let weak = Rc::downgrade(driver);
        use_key.connect_clicked(move |_| {
            if let Some(driver) = weak.upgrade() {
                driver.act(Action::SearchGifs(String::new()));
            }
        });
        gif_key.append(&use_key);
        gifs.append(&gif_key);
        let gif_search = awra::SearchEntry::new();
        gif_search.set_placeholder_text(t("Search GIFs via GIPHY"));
        gif_search.set_search_delay(400);
        let weak = Rc::downgrade(driver);
        gif_search.connect_search_changed(move |entry| {
            let query = entry.text();
            if let Some(driver) = weak.upgrade() {
                let current = driver.read(|app| app.gif_query.clone());
                if query.trim() != current.trim() {
                    driver.act(Action::SearchGifs(query));
                }
            }
        });
        gifs.append(&gif_search);
        let gif_status = muted("");
        gifs.append(&gif_status);
        let gif_grid = grid();
        let gif_more = awra::Button::with_label(&t("More GIFs"));
        gif_more.set_halign(awra::Align::Center);
        let weak = Rc::downgrade(driver);
        gif_more.connect_clicked(move |_| {
            if let Some(driver) = weak.upgrade() {
                driver.act(Action::MoreGifs);
            }
        });
        let gif_content = awra::Box::new(awra::Orientation::Vertical, 8);
        gif_content.append(&gif_grid);
        gif_content.append(&gif_more);
        let gif_scroll = awra::ScrolledWindow::new();
        gif_scroll.set_child(Some(&gif_content));
        gif_scroll.set_vexpand(true);
        gif_scroll.set_policy(awra::PolicyType::Automatic, awra::PolicyType::Automatic);
        gif_scroll.set_propagate_natural_width(false);
        gif_scroll.set_max_content_width(WIDTH);
        gifs.append(&gif_scroll);
        stack.add_named(&gifs, Some("gifs"));

        // Stickers: favourites, recent, received, then each pack.
        let sticker_list = awra::Box::new(awra::Orientation::Vertical, 10);
        let sticker_scroll = awra::ScrolledWindow::new();
        sticker_scroll.set_child(Some(&sticker_list));
        sticker_scroll.set_vexpand(true);
        sticker_scroll.set_policy(awra::PolicyType::Automatic, awra::PolicyType::Automatic);
        sticker_scroll.set_propagate_natural_width(false);
        sticker_scroll.set_max_content_width(WIDTH);
        stack.add_named(&sticker_scroll, Some("stickers"));

        // A tab shown asks the app for its contents, as the egui picker's do.
        let (weak, pages) = (Rc::downgrade(driver), stack.clone());
        tabs.connect_changed(move |_, index| {
            let Some((tab, name)) = TABS.get(index as usize).copied() else {
                return;
            };
            pages.set_visible_child_name(name);
            if let Some(driver) = weak.upgrade()
                && driver.read(|app| app.picker) != Some(tab)
            {
                driver.act(Action::TogglePicker(tab));
            }
        });
        let weak = Rc::downgrade(driver);
        popover.connect_closed(move |_| {
            if let Some(driver) = weak.upgrade()
                && driver.read(|app| app.picker.is_some())
            {
                driver.act(Action::ClosePicker);
            }
        });
        tabs.set_selected(0);

        Self {
            popover,
            tabs,
            stack,
            locale,
            driver: Rc::downgrade(driver),
            gif_key,
            gif_key_entry,
            gif_search,
            gif_status,
            gif_grid,
            gif_more,
            gif_tiles: Vec::new(),
            gif_shown: Vec::new(),
            sticker_list,
            sticker_tiles: Vec::new(),
            sticker_shown: None,
        }
    }

    /// Opens on the emoji.
    pub fn open(&self) {
        self.show(PickerTab::Emoji);
    }

    /// Opens on `tab`.
    pub fn show(&self, tab: PickerTab) {
        let index = TABS.iter().position(|(each, _)| *each == tab).unwrap_or(0);
        self.tabs.set_selected(index as u32);
        self.stack.set_visible_child_name(TABS[index].1);
        self.popover.popup();
    }

    pub fn sync(&mut self, app: &App) {
        self.sync_gifs(app);
        self.sync_stickers(app);
    }

    fn sync_gifs(&mut self, app: &App) {
        let t = |text: &'static str| gettext(self.locale, text).into_owned();
        let bad_key = app.gif_error.as_ref().is_some_and(|error| error.bad_key);
        let keyless = app.settings.effective_giphy_key().is_none() || bad_key;
        self.gif_key.set_visible(keyless);
        self.gif_search.set_visible(!keyless);
        if keyless && self.gif_key_entry.text().is_empty() && !app.settings.giphy_key.is_empty() {
            self.gif_key_entry.set_text(&app.settings.giphy_key);
        }
        let status = if app.gif_pending && app.gif_results.is_empty() {
            t("Searching…")
        } else if let Some(error) = &app.gif_error {
            error.message.clone()
        } else if app.gif_results.is_empty() && !keyless && !app.gif_query.is_empty() {
            t("No GIFs found")
        } else {
            String::new()
        };
        self.gif_status.set_visible(!status.is_empty());
        self.gif_status.set_text(&status);
        self.gif_more
            .set_visible(app.gif_next_offset.is_some() && !app.gif_results.is_empty());
        self.gif_more.set_sensitive(!app.gif_pending);

        let shown: Vec<(String, Option<PathBuf>)> = app
            .gif_results
            .iter()
            .map(|gif| (gif.id.clone(), gif.still.clone()))
            .collect();
        if shown != self.gif_shown {
            // Results only grow at their end while paging.
            let kept = self
                .gif_shown
                .iter()
                .zip(&shown)
                .take_while(|(a, b)| a == b)
                .count();
            if kept < self.gif_shown.len() {
                self.gif_grid.remove_all();
                self.gif_tiles.clear();
                for gif in &app.gif_results {
                    self.add_gif(gif);
                }
            } else {
                for gif in &app.gif_results[kept..] {
                    self.add_gif(gif);
                }
            }
            self.gif_shown = shown;
        }
        self.gif_tiles.iter_mut().for_each(Tile::fill);
    }

    fn add_gif(&mut self, gif: &Gif) {
        let (weak, picked, shown) = (self.driver.clone(), gif.clone(), self.popover.clone());
        let (button, tile) = tile(gif.still.clone(), awra::ContentFit::Cover, move || {
            if let Some(driver) = weak.upgrade() {
                driver.act(Action::SendGif(picked.clone()));
            }
            shown.popdown();
        });
        self.gif_grid.append(&button);
        self.gif_tiles.push(tile);
    }

    fn sync_stickers(&mut self, app: &App) {
        let t = |text: &'static str| gettext(self.locale, text).into_owned();
        let mut sections: Vec<(String, Vec<PathBuf>)> = Vec::new();
        for (title, stickers) in [
            (t("Favorites"), &app.stickers_saved),
            (t("Recent"), &app.stickers),
            (t("Received in chats"), &app.stickers_received),
        ] {
            if !stickers.is_empty() {
                sections.push((title, stickers.clone()));
            }
        }
        for pack in &app.sticker_packs {
            if !pack.stickers.is_empty() {
                sections.push((pack.name.clone(), pack.stickers.clone()));
            }
        }
        let key = (sections.is_empty() && app.stickers_pending, sections);
        if self.sticker_shown.as_ref() != Some(&key) {
            let (_, sections) = &key;
            while let Some(child) = self.sticker_list.first_child() {
                self.sticker_list.remove(&child);
            }
            self.sticker_tiles.clear();
            if sections.is_empty() {
                self.sticker_list.append(&muted(&if key.0 {
                    t("Loading stickers…")
                } else {
                    t("No stickers yet. Stickers you send, receive or save on your phone show up here.")
                }));
            }
            for (title, stickers) in sections {
                let heading = awra::Label::new(Some(title));
                heading.set_xalign(0.0);
                heading.set_weight(600);
                self.sticker_list.append(&heading);
                let grid = grid();
                for path in stickers {
                    let (weak, picked, shown) =
                        (self.driver.clone(), path.clone(), self.popover.clone());
                    let (button, tile) =
                        tile(Some(path.clone()), awra::ContentFit::Contain, move || {
                            if let Some(driver) = weak.upgrade() {
                                driver.act(Action::SendSticker(picked.clone()));
                            }
                            shown.popdown();
                        });
                    grid.append(&button);
                    self.sticker_tiles.push(tile);
                }
                self.sticker_list.append(&grid);
            }
            self.sticker_shown = Some(key);
        }
        self.sticker_tiles.iter_mut().for_each(Tile::fill);
    }
}
