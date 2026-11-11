//! The composer's emoji picker: Awra's picker, filled from the same emoji
//! data the egui picker reads, a section per Unicode group.

use awra::prelude::*;

/// The groups' titles and icons, in the picker's order.
const GROUPS: [(emojis::Group, &str, &str); 9] = [
    (
        emojis::Group::SmileysAndEmotion,
        "Smileys & Emotion",
        "emoji-people-symbolic",
    ),
    (
        emojis::Group::PeopleAndBody,
        "People & Body",
        "emoji-body-symbolic",
    ),
    (
        emojis::Group::AnimalsAndNature,
        "Animals & Nature",
        "emoji-nature-symbolic",
    ),
    (
        emojis::Group::FoodAndDrink,
        "Food & Drink",
        "emoji-food-symbolic",
    ),
    (
        emojis::Group::TravelAndPlaces,
        "Travel & Places",
        "emoji-travel-symbolic",
    ),
    (
        emojis::Group::Activities,
        "Activities",
        "emoji-activities-symbolic",
    ),
    (emojis::Group::Objects, "Objects", "emoji-objects-symbolic"),
    (emojis::Group::Symbols, "Symbols", "emoji-symbols-symbolic"),
    (emojis::Group::Flags, "Flags", "emoji-flags-symbolic"),
];

/// A popover with the picker, anchored to `anchor`; a pick goes into
/// `composer` at its cursor.
pub fn popover(anchor: &awra::Button, composer: &awra::Composer) -> awra::Popover {
    let picker = awra::EmojiPicker::new();
    for (group, title, icon) in GROUPS {
        let store = awra::ListStore::new::<awra::EmojiItem>();
        for emoji in group.emojis() {
            let item = awra::EmojiItem::new(emoji.as_str(), emoji.name(), Some(emoji.as_str()));
            let shortcodes: Vec<&str> = emoji.shortcodes().collect();
            item.set_keywords(&shortcodes);
            store.append(&item);
        }
        picker.add_section(&format!("{group:?}"), title, Some(icon), &store);
    }
    let popover = awra::Popover::new();
    popover.set_child(Some(&picker));
    popover.set_parent(anchor);
    popover.set_position(awra::PositionType::Top);
    let target = composer.clone();
    let shown = popover.clone();
    picker.connect_picked(move |_, item| {
        if let Some(text) = item.text() {
            target.buffer().insert_at_cursor(&text);
        }
        shown.popdown();
        target.view().grab_focus();
    });
    popover
}
