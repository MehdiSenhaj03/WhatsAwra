//! The open chat: its header, the messages (Awra's conversation view, one
//! bubble per message under a divider for each day), who is typing, and
//! the composer.

use std::cell::RefCell;
use std::collections::HashSet;
use std::path::PathBuf;
use std::rc::Rc;

use awra::prelude::*;

use super::{Driver, Op};
use whatsawra::app::App;
use whatsawra::i18n::{Locale, gettext};
use whatsawra::model::{Action, Chat, Content, Delivery, Message};

/// One row of the message list.
#[derive(Clone, Debug, PartialEq)]
enum Item {
    /// The date above the messages of one day.
    Day {
        key: String,
        label: String,
    },
    /// Where the unread messages start.
    Unread {
        label: String,
    },
    Message(Box<Bubble>),
    /// A line from WhatsApp itself (a deleted message, a timer changed).
    Notice {
        id: String,
        text: String,
    },
}

impl Item {
    fn key(&self) -> &str {
        match self {
            Self::Day { key, .. } => key,
            Self::Unread { .. } => "unread",
            Self::Message(bubble) => &bubble.id,
            Self::Notice { id, .. } => id,
        }
    }
}

/// What one bubble shows.
#[derive(Clone, Debug, PartialEq)]
struct Bubble {
    id: String,
    from_me: bool,
    /// The sender's name and colour, above the first of a run in a group.
    sender: Option<(String, String)>,
    /// The sender's picture beside the first of a run in a group, if any.
    face: Option<(String, Option<PathBuf>)>,
    /// The first of a run from one sender, drawn with a tail.
    tail: bool,
    /// Leaves room for a face that a later bubble of the run shows.
    indent: bool,
    quote: Option<(String, String)>,
    markup: String,
    big: bool,
    forwarded: bool,
    stamp: String,
    status: Option<Delivery>,
    edited: bool,
    reactions: Vec<(String, usize, bool)>,
    media: Option<super::media::MediaView>,
    /// The media should download as soon as the row shows.
    fetch: bool,
}

#[derive(Clone)]
struct RowWidgets {
    divider: awra::Divider,
    notice: awra::Label,
    line: awra::Box,
    face: awra::Avatar,
    card: awra::MessageBubble,
    sender: awra::Label,
    forwarded: awra::Label,
    quote: awra::Box,
    quote_name: awra::Label,
    quote_text: awra::Label,
    body: awra::Label,
    meta: awra::Label,
    reactions: awra::Box,
    media: super::media::MediaWidgets,
    /// Messages whose media this view has asked for.
    fetched: Rc<RefCell<HashSet<String>>>,
    /// The message the row shows, for its menu and reactions.
    bound: Rc<RefCell<Option<String>>>,
    driver: std::rc::Weak<Driver>,
}

pub struct ConversationPane {
    pub root: awra::Stack,
    /// The open chat's header, for the window's header.
    pub header: awra::Box,
    avatar: awra::Avatar,
    title: awra::Label,
    subtitle: awra::Label,
    view: awra::ConversationView,
    store: awra::ListStore,
    items: Rc<RefCell<Vec<Item>>>,
    typing: awra::TypingIndicator,
    composer: awra::Composer,
    attachments: super::composer::Attachments,
    attachments_given: bool,
    recording: super::composer::Recording,
    composer_banner: Option<String>,
    /// The chat the view shows, to start each chat at its newest message.
    shown_chat: Option<String>,
    shown_avatar: Option<PathBuf>,
    asked: HashSet<String>,
    picker: Rc<RefCell<Option<super::picker::Picker>>>,
    emoji: awra::Button,
}

impl ConversationPane {
    pub fn build(driver: &Rc<Driver>, locale: Locale) -> Self {
        let root = awra::Stack::new();
        root.set_transition_type(awra::StackTransitionType::Crossfade);

        let empty = awra::EmptyState::new();
        empty.set_icon_name(Some("awra-chats-symbolic"));
        empty.set_title(Some(gettext(locale, "WhatsAwra").as_ref()));
        empty.set_description(Some(
            gettext(locale, "Pick a chat to read and write its messages.").as_ref(),
        ));
        root.add_named(&empty, Some("empty"));

        let chat = awra::Box::new(awra::Orientation::Vertical, 0);

        // The header: who, their status, and the chat's actions. It sits in
        // the window's own header, above the messages (the shell puts it there).
        let header = awra::Box::new(awra::Orientation::Horizontal, 12);
        header.set_hexpand(true);
        header.set_margin_start(16);
        header.set_margin_end(8);
        let avatar = awra::Avatar::new(36, None);
        avatar.set_valign(awra::Align::Center);
        header.append(&avatar);
        let names = awra::Box::new(awra::Orientation::Vertical, 0);
        names.set_hexpand(true);
        names.set_valign(awra::Align::Center);
        let title = awra::Label::new(None);
        title.set_xalign(0.0);
        title.set_weight(600);
        title.set_ellipsize(awra::pango::EllipsizeMode::End);
        let subtitle = awra::Label::new(None);
        subtitle.set_xalign(0.0);
        subtitle.set_typography(awra::Typography::Muted);
        subtitle.set_ellipsize(awra::pango::EllipsizeMode::End);
        names.append(&title);
        names.append(&subtitle);
        header.append(&names);
        let action_button = |icon: &str, tooltip: &'static str, action: fn(String) -> Action| {
            let button = awra::Button::from_icon_name(icon);
            button.set_appearance(awra::ButtonAppearance::Ghost);
            button.set_tooltip_text(Some(gettext(locale, tooltip).as_ref()));
            button.set_valign(awra::Align::Center);
            let weak = Rc::downgrade(driver);
            button.connect_clicked(move |_| {
                if let Some(driver) = weak.upgrade()
                    && let Some(chat) = driver.read(|app| app.open_chat.clone())
                {
                    driver.act(action(chat));
                }
            });
            header.append(&button);
            button
        };
        action_button(
            "awra-video-call-symbolic",
            "Video call",
            Action::StartVideoCall,
        );
        action_button("awra-call-symbolic", "Voice call", Action::StartCall);
        action_button("system-search-symbolic", "Search messages", |_| {
            Action::OpenChatSearch
        });
        action_button("info-outline-symbolic", "Chat info", Action::OpenInfo);

        // The messages.
        let items: Rc<RefCell<Vec<Item>>> = Rc::default();
        let store = awra::ListStore::new::<awra::StringObject>();
        let factory = awra::SignalListItemFactory::new();
        let weak = Rc::downgrade(driver);
        let fetched: Rc<RefCell<HashSet<String>>> = Rc::default();
        factory.connect_setup(move |_, item| {
            let (root, widgets) = row_widgets(&weak, &fetched);
            root.set_rust_data("row", widgets);
            item.set_child(Some(&root));
        });
        let bound = items.clone();
        factory.connect_bind(move |_, item| {
            let Some(child) = item.child() else { return };
            let Some(widgets) = child.rust_data::<RowWidgets>("row") else {
                return;
            };
            if let Some(row) = bound.borrow().get(item.position() as usize) {
                bind_row(&widgets, row);
            }
        });
        let view = awra::ConversationView::new(Some(store.clone()), Some(factory));
        awra::add_style_role(&view, "entry-hover");
        view.set_vexpand(true);
        let weak = Rc::downgrade(driver);
        view.connect_edge_reached(move |_, edge| {
            if edge != awra::PositionType::Top {
                return;
            }
            if let Some(driver) = weak.upgrade()
                && let Some(chat) = driver.read(|app| app.open_chat.clone())
            {
                driver.act(Action::LoadOlder {
                    chat,
                    explicit: true,
                });
            }
        });
        chat.append(&view);

        let typing = awra::TypingIndicator::new();
        typing.set_margin_start(20);
        chat.append(&typing);

        // The composer.
        let composer = awra::Composer::new();
        composer.set_margin_start(12);
        composer.set_margin_end(12);
        composer.set_margin_bottom(12);
        composer.set_placeholder_text(Some(gettext(locale, "Type a message").as_ref()));
        composer.set_max_lines(8);
        let attach = awra::Button::from_icon_name("list-add-symbolic");
        attach.set_appearance(awra::ButtonAppearance::Toolbar);
        attach.set_tooltip_text(Some(gettext(locale, "Attach").as_ref()));
        let weak = Rc::downgrade(driver);
        attach.connect_clicked(move |_| {
            if let Some(driver) = weak.upgrade() {
                driver.act(Action::Attach { documents: false });
            }
        });
        composer.set_start_widget(Some(&attach));
        let emoji = awra::Button::from_icon_name("face-smile-symbolic");
        emoji.set_appearance(awra::ButtonAppearance::Toolbar);
        emoji.set_tooltip_text(Some(gettext(locale, "Emoji").as_ref()));
        // The picker (emoji, GIFs, stickers), made on its first opening: an
        // emoji goes in at the composer's cursor, the rest is sent at once.
        let picker: Rc<RefCell<Option<super::picker::Picker>>> = Rc::default();
        let (anchor, target, made) = (emoji.clone(), composer.clone(), picker.clone());
        let weak = Rc::downgrade(driver);
        emoji.connect_clicked(move |_| {
            let Some(driver) = weak.upgrade() else { return };
            let mut made = made.borrow_mut();
            let picker = made.get_or_insert_with(|| {
                super::picker::Picker::build(&driver, locale, &anchor, &target)
            });
            driver.read(|app| picker.sync(app));
            picker.open();
        });
        let recording = super::composer::Recording::build(driver, locale);
        let ends = awra::Box::new(awra::Orientation::Horizontal, 0);
        ends.append(&emoji);
        ends.append(&recording.mic);
        composer.set_end_widget(Some(&ends));
        let attachments = super::composer::Attachments::new();
        let weak = Rc::downgrade(driver);
        composer.connect_text_notify(move |composer| {
            let text = composer.text();
            if let Some(driver) = weak.upgrade() {
                driver.edit(move |app| {
                    if app.composer != text {
                        app.composer = text;
                        app.frontend_keystroke();
                    }
                });
            }
        });
        let weak = Rc::downgrade(driver);
        composer.connect_submit(move |composer| {
            let text = composer.text();
            let Some(driver) = weak.upgrade() else { return };
            let action = driver.read(|app| {
                let chat = app.open_chat.clone()?;
                if app.pending.is_empty() {
                    (!text.trim().is_empty()).then(|| Action::SendText {
                        chat,
                        text: text.clone(),
                        quoting: app.reply_to.clone(),
                    })
                } else {
                    // Staged attachments go with the text as their caption.
                    Some(Action::SendPending {
                        account: app.account().id.clone(),
                        mentions: app.composer_mentions(),
                        chat,
                        caption: text.clone(),
                    })
                }
            });
            let Some(action) = action else { return };
            composer.set_text(None);
            driver.edit(|app| app.composer.clear());
            driver.act(action);
        });
        let weak = Rc::downgrade(driver);
        composer.connect_banner_closed(move |_| {
            if let Some(driver) = weak.upgrade() {
                let editing = driver.read(|app| app.editing.is_some());
                driver.act(if editing {
                    Action::CancelEdit
                } else {
                    Action::CancelReply
                });
            }
        });
        chat.append(&composer);
        chat.append(&recording.bar);
        root.add_named(&chat, Some("chat"));

        Self {
            root,
            header,
            avatar,
            title,
            subtitle,
            view,
            store,
            items,
            typing,
            composer,
            composer_banner: None,
            attachments,
            attachments_given: false,
            recording,
            shown_chat: None,
            shown_avatar: None,
            asked: HashSet::new(),
            picker,
            emoji,
        }
    }

    pub fn sync(&mut self, driver: &Rc<Driver>, app: &App) {
        let chat = app.current_chat().filter(|_| app.chat_view());
        self.header.set_visible(chat.is_some());
        let Some(chat) = chat else {
            if self.root.visible_child_name().as_deref() != Some("empty") {
                self.root.set_visible_child_name("empty");
            }
            self.shown_chat = None;
            return;
        };
        if self.root.visible_child_name().as_deref() != Some("chat") {
            self.root.set_visible_child_name("chat");
        }
        let switched = self.shown_chat.as_deref() != Some(chat.id.as_str());
        self.sync_header(app, chat, switched);
        self.sync_messages(driver, app, chat, switched);
        self.sync_composer(driver, app, chat, switched);
        self.sync_picker(driver, app);
        self.shown_chat = Some(chat.id.clone());
    }

    fn sync_header(&mut self, app: &App, chat: &Chat, switched: bool) {
        let title = app.chat_title(chat);
        self.title.set_text(&title);
        self.avatar.set_text(Some(&title));
        self.avatar.set_shape(if chat.is_group() {
            awra::AvatarShape::Rounded
        } else {
            awra::AvatarShape::Circle
        });
        let picture = app.avatar_path(&chat.id);
        if switched || picture != self.shown_avatar {
            let texture = picture.as_deref().and_then(super::pictures::file);
            self.avatar.set_custom_image(texture.as_ref());
            self.shown_avatar = picture;
        }
        let (subtitle, tone) = whatsawra::present::chat_subtitle(app, chat);
        self.subtitle.set_text(&subtitle);
        self.subtitle.set_visible(!subtitle.is_empty());
        self.subtitle.set_typography(match tone {
            whatsawra::present::Tone::Accent => awra::Typography::Label,
            whatsawra::present::Tone::Secondary => awra::Typography::Muted,
        });
        let typing = !app.typing_in(&chat.id).is_empty();
        self.typing.set_active(typing);
        if typing {
            self.typing.set_label(Some(&subtitle));
        }
    }

    fn sync_messages(&mut self, driver: &Rc<Driver>, app: &App, chat: &Chat, switched: bool) {
        let messages: &[Message] = app
            .conversations
            .get(&chat.id)
            .map_or(&[], |conversation| conversation.messages.as_slice());
        let items = items_of(app, chat, messages);

        let mut wanted = Vec::new();
        for item in &items {
            if let Item::Message(bubble) = item
                && let Some((id, None)) = &bubble.face
                && app.avatar_unasked(id)
                && self.asked.insert(id.clone())
            {
                wanted.push(id.clone());
            }
        }
        if !wanted.is_empty() {
            driver.later(Op::Edit(Box::new(move |app: &mut App| {
                for id in wanted {
                    app.avatar(&id);
                }
            })));
        }

        let old = self.items.borrow().clone();
        *self.items.borrow_mut() = items.clone();
        let object = |item: &Item| awra::Object::new(awra::StringObject(item.key().to_owned()));
        if switched {
            let objects: Vec<awra::Object> = items.iter().map(object).collect();
            self.store.splice(0, self.store.n_items(), &objects);
            self.view.scroll_to_end(false);
            return;
        }
        let list = self.view.delegate();
        super::rows::update(&self.store, &old, &items, Item::key, |position| {
            list.rebind(position)
        });
    }

    fn sync_composer(&mut self, driver: &Rc<Driver>, app: &App, chat: &Chat, switched: bool) {
        let staged = self.attachments.sync(driver, app);
        // The composer keeps a section's room while it has one: the strip
        // is given only while something is staged.
        if staged != self.attachments_given {
            if staged {
                self.composer.set_attachments(Some(&self.attachments.root));
            } else {
                self.composer.set_attachments(None::<&awra::Widget>);
            }
            self.attachments_given = staged;
        }
        self.composer.set_can_send(staged);
        let recording = self
            .recording
            .sync(app, app.composer.trim().is_empty() && !staged);
        self.composer.set_visible(!recording);
        if self.composer.text() != app.composer {
            self.composer.set_text(Some(&app.composer));
        }
        self.composer.set_sensitive(chat.can_send());
        let banner = if app.editing.is_some() {
            Some(gettext(app.locale, "Editing message").into_owned())
        } else {
            app.reply_to.as_ref().map(|target| {
                let source = target.source_chat(&chat.id);
                let quoted = app
                    .conversations
                    .get(source)
                    .and_then(|conversation| conversation.message(&target.id));
                let who = match quoted {
                    Some(message) if message.from_me => gettext(app.locale, "You").into_owned(),
                    Some(message) => {
                        app.display_name_or(&message.sender, message.sender_name.as_deref())
                    }
                    None => String::new(),
                };
                let text = quoted
                    .map(|message| app.message_text(message).replace('\n', " "))
                    .unwrap_or_default();
                format!("{who}: {text}")
            })
        };
        if banner != self.composer_banner || switched {
            match &banner {
                Some(text) => self.composer.show_banner(text, true),
                None => self.composer.set_banner(None::<&awra::Widget>),
            }
            self.composer_banner = banner;
        }
        if switched {
            self.composer.view().grab_focus();
        }
    }
}

/// The rows of a chat: its messages, a divider before each day and before
/// the first unread message.
fn items_of(app: &App, chat: &Chat, messages: &[Message]) -> Vec<Item> {
    let mut items = Vec::with_capacity(messages.len() + 8);
    let mut day = None;
    let unread_from = app
        .unread_divider
        .as_ref()
        .filter(|divider| divider.chat == chat.id)
        .and_then(|divider| first_unread(messages, divider.count));
    let mine = app.our_ids();
    for (index, message) in messages.iter().enumerate() {
        let key = whatsawra::util::day_key(message.timestamp);
        if key != day {
            day = key;
            items.push(Item::Day {
                key: format!("day:{}", message.id),
                label: whatsawra::util::day_label(app.locale, message.timestamp),
            });
        }
        if unread_from.as_deref() == Some(message.id.as_str()) {
            items.push(Item::Unread {
                label: gettext(app.locale, "Unread messages").into_owned(),
            });
        }
        if let Some(text) = notice_of(app, message) {
            items.push(Item::Notice {
                id: message.id.clone(),
                text,
            });
            continue;
        }
        let previous = index.checked_sub(1).and_then(|at| messages.get(at));
        let next = messages.get(index + 1);
        let same_sender = |other: Option<&Message>| {
            other.is_some_and(|other| {
                other.sender == message.sender
                    && other.from_me == message.from_me
                    && whatsawra::util::day_key(other.timestamp) == key
                    && notice_of(app, other).is_none()
            })
        };
        let first_of_run = !same_sender(previous);
        let last_of_run = !same_sender(next);
        let group_incoming = chat.is_group() && !message.from_me;
        let name = || app.display_name_or(&message.sender, message.sender_name.as_deref());
        let sender =
            (group_incoming && first_of_run).then(|| (name(), sender_colour(&message.sender)));
        let face = (group_incoming && last_of_run)
            .then(|| (message.sender.clone(), app.avatar_path(&message.sender)));
        let quote = message.quoted.as_ref().map(|quoted| {
            let who = if mine.contains(&quoted.sender.as_str()) {
                gettext(app.locale, "You").into_owned()
            } else {
                app.display_name_or(&quoted.sender, quoted.sender_name.as_deref())
            };
            (who, quoted.summary_in(app.locale).replace('\n', " "))
        });
        let (markup, big) = body_of(app, message);
        let (media, fetch) = match super::media::view_of(app, message) {
            Some((media, fetch)) => (Some(media), fetch),
            None => (None, false),
        };
        let mut reactions: Vec<(String, usize, bool)> = Vec::new();
        for reaction in &message.reactions {
            match reactions
                .iter_mut()
                .find(|(emoji, _, _)| *emoji == reaction.emoji)
            {
                Some((_, count, me)) => {
                    *count += 1;
                    *me |= reaction.from_me;
                }
                None => reactions.push((reaction.emoji.clone(), 1, reaction.from_me)),
            }
        }
        items.push(Item::Message(Box::new(Bubble {
            id: message.id.clone(),
            from_me: message.from_me,
            sender,
            tail: first_of_run,
            face,
            indent: group_incoming && !last_of_run,
            quote,
            markup,
            big,
            forwarded: message.forwarded,
            stamp: whatsawra::util::clock(message.timestamp),
            status: message.from_me.then_some(message.status),
            edited: message.edited,
            reactions,
            media,
            fetch,
        })));
    }
    items
}

/// The first of the last `count` incoming messages, where the unread ones
/// start.
fn first_unread(messages: &[Message], count: u32) -> Option<String> {
    if count == 0 {
        return None;
    }
    let mut seen = 0;
    for message in messages.iter().rev() {
        if !message.from_me {
            seen += 1;
            if seen == count {
                return Some(message.id.clone());
            }
        }
    }
    None
}

/// The text of a line WhatsApp writes into the chat itself.
fn notice_of(app: &App, message: &Message) -> Option<String> {
    match &message.content {
        Content::Revoked => Some(if message.from_me {
            gettext(app.locale, "You deleted this message").into_owned()
        } else {
            gettext(app.locale, "This message was deleted").into_owned()
        }),
        Content::DisappearingTimer { .. } => Some(app.message_text(message)),
        _ => None,
    }
}

/// A bubble's text as markup, and whether it is emoji alone (drawn large).
fn body_of(app: &App, message: &Message) -> (String, bool) {
    // Read once per message and text: every turn reads every loaded message.
    thread_local! {
        static MARKUP: RefCell<std::collections::HashMap<String, (u64, String, bool)>> =
            RefCell::default();
    }
    let mentions = app.mention_list(message);
    let key = {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        let shown = match &message.content {
            Content::Text { text, .. } | Content::Interactive { text, .. } => Some(text.as_str()),
            Content::Image { caption, .. }
            | Content::Video { caption, .. }
            | Content::Document { caption, .. } => caption.as_deref(),
            _ => None,
        };
        shown.hash(&mut hasher);
        std::mem::discriminant(&message.content).hash(&mut hasher);
        // What other content says may change (a poll, a call): read it again.
        if shown.is_none() {
            app.message_text(message).hash(&mut hasher);
        }
        for mention in &mentions {
            mention.user.hash(&mut hasher);
            mention.name.hash(&mut hasher);
        }
        hasher.finish()
    };
    if let Some((markup, big)) = MARKUP.with(|cache| {
        cache
            .borrow()
            .get(&message.id)
            .filter(|(hash, _, _)| *hash == key)
            .map(|(_, markup, big)| (markup.clone(), *big))
    }) {
        return (markup, big);
    }
    let (markup, big) = read_body(app, message, &mentions);
    MARKUP.with(|cache| {
        cache
            .borrow_mut()
            .insert(message.id.clone(), (key, markup.clone(), big))
    });
    (markup, big)
}

fn read_body(
    app: &App,
    message: &Message,
    mentions: &[whatsawra::markup::Mention],
) -> (String, bool) {
    let mentions = mentions.to_vec();
    match &message.content {
        Content::Text { text, .. } | Content::Interactive { text, .. } => {
            let big = is_emoji_only(text);
            (
                whatsawra::markup::pango(text, &mentions, "#3b9cff", "#8a8a8a"),
                big,
            )
        }
        // The media draws itself; only a caption goes under it.
        Content::Image { caption, .. }
        | Content::Video { caption, .. }
        | Content::Document { caption, .. } => (
            caption
                .as_deref()
                .filter(|caption| !caption.is_empty())
                .map(|caption| whatsawra::markup::pango(caption, &mentions, "#3b9cff", "#8a8a8a"))
                .unwrap_or_default(),
            false,
        ),
        Content::Audio { .. } | Content::Sticker { .. } => (String::new(), false),
        _ => (
            format!("<i>{}</i>", escape(&app.message_text(message))),
            false,
        ),
    }
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn is_emoji_only(text: &str) -> bool {
    let trimmed = text.trim();
    !trimmed.is_empty()
        && trimmed.chars().count() <= 12
        && trimmed.chars().all(|c| {
            c.is_whitespace()
                || matches!(c as u32, 0x200d | 0xfe0f | 0x20e3)
                || (c as u32) >= 0x1f000
                || (0x2600..0x27c0).contains(&(c as u32))
        })
}

/// A sender's name colour, the same for them everywhere.
fn sender_colour(id: &str) -> String {
    let hue = whatsawra::util::hue(id) / 360.0;
    let (r, g, b) = hsl(hue, 0.55, 0.55);
    format!("#{r:02x}{g:02x}{b:02x}")
}

fn hsl(hue: f32, saturation: f32, lightness: f32) -> (u8, u8, u8) {
    let hue = hue.rem_euclid(1.0) * 6.0;
    let chroma = (1.0 - (2.0 * lightness - 1.0).abs()) * saturation;
    let x = chroma * (1.0 - (hue % 2.0 - 1.0).abs());
    let (r, g, b) = match hue as u32 {
        0 => (chroma, x, 0.0),
        1 => (x, chroma, 0.0),
        2 => (0.0, chroma, x),
        3 => (0.0, x, chroma),
        4 => (x, 0.0, chroma),
        _ => (chroma, 0.0, x),
    };
    let m = lightness - chroma / 2.0;
    let byte = |v: f32| ((v + m).clamp(0.0, 1.0) * 255.0).round() as u8;
    (byte(r), byte(g), byte(b))
}

fn row_widgets(
    driver: &std::rc::Weak<Driver>,
    fetched: &Rc<RefCell<HashSet<String>>>,
) -> (awra::Box, RowWidgets) {
    let root = awra::Box::new(awra::Orientation::Vertical, 0);
    root.set_margin_start(16);
    root.set_margin_end(16);

    let divider = awra::Divider::new(None);
    divider.set_margin_top(10);
    divider.set_margin_bottom(6);
    root.append(&divider);

    let notice = awra::Label::new(None);
    notice.set_typography(awra::Typography::Muted);
    notice.set_halign(awra::Align::Center);
    notice.set_margin_top(4);
    notice.set_margin_bottom(4);
    notice.set_wrap(true);
    root.append(&notice);

    let line = awra::Box::new(awra::Orientation::Horizontal, 8);
    awra::add_style_role(&line, "conversation-entry");
    line.set_margin_top(1);
    line.set_margin_bottom(1);
    let face = awra::Avatar::new(28, None);
    face.set_valign(awra::Align::End);
    line.append(&face);

    let card = awra::MessageBubble::new();
    let content = awra::Box::new(awra::Orientation::Vertical, 2);
    let sender = awra::Label::new(None);
    sender.set_xalign(0.0);
    sender.set_weight(600);
    content.append(&sender);
    let forwarded = awra::Label::new(None);
    forwarded.set_xalign(0.0);
    forwarded.set_typography(awra::Typography::Muted);
    content.append(&forwarded);

    let quote = awra::Box::new(awra::Orientation::Vertical, 0);
    awra::add_style_role(&quote, "chosen-row");
    quote.set_margin_bottom(2);
    let quote_name = awra::Label::new(None);
    quote_name.set_xalign(0.0);
    quote_name.set_weight(600);
    let quote_text = awra::Label::new(None);
    quote_text.set_xalign(0.0);
    quote_text.set_typography(awra::Typography::Muted);
    quote_text.set_ellipsize(awra::pango::EllipsizeMode::End);
    quote_text.set_max_width_chars(48);
    quote.append(&quote_name);
    quote.append(&quote_text);
    content.append(&quote);

    let media = super::media::MediaWidgets::build(driver);
    content.append(&media.root);

    let body = awra::Label::new(None);
    body.set_xalign(0.0);
    body.set_wrap(true);
    body.set_wrap_mode(awra::WrapMode::WordChar);
    body.set_max_width_chars(60);
    body.set_selectable(true);
    content.append(&body);

    let meta = awra::Label::new(None);
    meta.set_halign(awra::Align::End);
    meta.set_typography(awra::Typography::Muted);
    meta.set_tabular_digits(true);
    content.append(&meta);
    card.set_child(Some(&content));
    line.append(&card);
    root.append(&line);

    let reactions = awra::Box::new(awra::Orientation::Horizontal, 4);
    reactions.set_margin_bottom(2);
    root.append(&reactions);

    let bound: Rc<RefCell<Option<String>>> = Rc::default();
    // A right click anywhere on the message opens its menu.
    let menu = awra::GestureClick::new();
    menu.set_button(3);
    let (weak, shown, anchor) = (driver.clone(), bound.clone(), line.clone());
    menu.connect_pressed(move |_, _, x, y| {
        let (Some(driver), Some(id)) = (weak.upgrade(), shown.borrow().clone()) else {
            return;
        };
        let entries = driver.read(|app| super::menus::message(&driver, app, &id));
        if !entries.is_empty() {
            awra::menu::popup_at(&anchor, x, y, entries);
        }
    });
    line.add_controller(menu);
    (
        root,
        RowWidgets {
            bound,
            driver: driver.clone(),
            divider,
            notice,
            line,
            face,
            card,
            sender,
            forwarded,
            quote,
            quote_name,
            quote_text,
            body,
            meta,
            reactions,
            media,
            fetched: fetched.clone(),
        },
    )
}

fn bind_row(widgets: &RowWidgets, item: &Item) {
    *widgets.bound.borrow_mut() = match item {
        Item::Message(bubble) => Some(bubble.id.clone()),
        _ => None,
    };
    let show = |divider: bool, notice: bool, line: bool| {
        widgets.divider.set_visible(divider);
        widgets.notice.set_visible(notice);
        widgets.line.set_visible(line);
        widgets.reactions.set_visible(line);
    };
    match item {
        Item::Day { label, .. } => {
            show(true, false, false);
            widgets.divider.set_label(Some(label));
            widgets
                .divider
                .set_appearance(awra::DividerAppearance::Neutral);
            widgets
                .divider
                .set_label_position(awra::DividerLabelPosition::Center);
        }
        Item::Unread { label } => {
            show(true, false, false);
            widgets.divider.set_label(Some(label));
            widgets
                .divider
                .set_appearance(awra::DividerAppearance::Danger);
            widgets
                .divider
                .set_label_position(awra::DividerLabelPosition::End);
        }
        Item::Notice { text, .. } => {
            show(false, true, false);
            widgets.notice.set_text(text);
        }
        Item::Message(bubble) => {
            show(false, false, true);
            bind_bubble(widgets, bubble);
        }
    }
}

fn bind_bubble(widgets: &RowWidgets, bubble: &Bubble) {
    let align = if bubble.from_me {
        awra::Align::End
    } else {
        awra::Align::Start
    };
    widgets.line.set_halign(align);
    widgets.reactions.set_halign(align);
    widgets.card.set_outgoing(bubble.from_me);
    widgets.card.set_tail(bubble.tail);
    widgets
        .face
        .set_visible(bubble.face.is_some() || bubble.indent);
    widgets
        .face
        .set_opacity(if bubble.face.is_some() { 1.0 } else { 0.0 });
    if let Some((id, picture)) = &bubble.face {
        let name = bubble
            .sender
            .as_ref()
            .map_or(id.as_str(), |(name, _)| name.as_str());
        widgets.face.set_text(Some(name));
        let texture = picture.as_deref().and_then(super::pictures::file);
        widgets.face.set_custom_image(texture.as_ref());
    }
    match &bubble.sender {
        Some((name, colour)) => {
            widgets.sender.set_visible(true);
            widgets.sender.set_markup(&format!(
                "<span foreground=\"{colour}\">{}</span>",
                escape(name)
            ));
        }
        None => widgets.sender.set_visible(false),
    }
    widgets.forwarded.set_visible(bubble.forwarded);
    if bubble.forwarded {
        widgets.forwarded.set_text("↪ Forwarded");
    }
    match &bubble.quote {
        Some((name, text)) => {
            widgets.quote.set_visible(true);
            widgets.quote_name.set_text(name);
            widgets.quote_text.set_text(text);
        }
        None => widgets.quote.set_visible(false),
    }
    widgets.body.set_visible(!bubble.markup.is_empty());
    widgets.body.set_markup(&bubble.markup);
    let thumbnail = widgets.driver.upgrade().and_then(|driver| {
        driver.read(|app| {
            let chat = app.open_chat.as_deref()?;
            app.conversations
                .get(chat)?
                .message(&bubble.id)?
                .thumbnail
                .clone()
        })
    });
    widgets
        .media
        .bind(&bubble.id, bubble.media.as_ref(), thumbnail.as_deref());
    if bubble.fetch
        && let Some(driver) = widgets.driver.upgrade()
        && let Some(chat) = driver.read(|app| app.open_chat.clone())
    {
        super::media::fetch(&driver, &widgets.fetched, &chat, &bubble.id);
    }
    widgets.body.set_scale(if bubble.big { 2.4 } else { 1.0 });
    let tick = match bubble.status {
        Some(Delivery::Pending) => " 🕓",
        Some(Delivery::Sent) => " ✓",
        Some(Delivery::Delivered) => " ✓✓",
        Some(Delivery::Read | Delivery::Played) => " ✓✓",
        Some(Delivery::Failed) => " ⚠",
        _ => "",
    };
    let edited = if bubble.edited { "edited · " } else { "" };
    let read = matches!(bubble.status, Some(Delivery::Read | Delivery::Played));
    if read {
        widgets.meta.set_markup(&format!(
            "{edited}{}<span foreground=\"#3b9cff\">{tick}</span>",
            escape(&bubble.stamp)
        ));
    } else {
        widgets
            .meta
            .set_text(&format!("{edited}{}{tick}", bubble.stamp));
    }
    while let Some(child) = widgets.reactions.first_child() {
        widgets.reactions.remove(&child);
    }
    for (emoji, count, mine) in &bubble.reactions {
        let chip = awra::Chip::new(None);
        chip.set_emoji(Some(emoji));
        if *count > 1 {
            chip.set_count(*count as i32);
        }
        chip.set_active(*mine);
        chip.set_toggleable(false);
        // Our own reaction comes off; another joins it.
        let (weak, message) = (widgets.driver.clone(), bubble.id.clone());
        let emoji = if *mine { String::new() } else { emoji.clone() };
        chip.connect_activate(move |_| {
            if let Some(driver) = weak.upgrade()
                && let Some(chat) = driver.read(|app| app.open_chat.clone())
            {
                driver.act(Action::React {
                    chat,
                    message: message.clone(),
                    emoji: emoji.clone(),
                });
            }
        });
        widgets.reactions.append(&chip);
    }
}

impl ConversationPane {
    /// Opens the newest message's menu in the middle of the view, as a right
    /// click on it would.
    pub fn open_menu(&self, driver: &Rc<Driver>) {
        let id = self
            .items
            .borrow()
            .iter()
            .rev()
            .find_map(|item| match item {
                Item::Message(bubble) => Some(bubble.id.clone()),
                _ => None,
            });
        let Some(id) = id else { return };
        let entries = driver.read(|app| super::menus::message(driver, app, &id));
        let (width, height) = (self.view.width() as f64, self.view.height() as f64);
        awra::menu::popup_at(&self.view, width * 0.6, height * 0.6, entries);
    }
}

impl ConversationPane {
    /// Keeps the open picker current, and opens it on the tab the app asks
    /// for (a shortcut, a sample).
    fn sync_picker(&mut self, driver: &Rc<Driver>, app: &App) {
        let Ok(mut picker) = self.picker.try_borrow_mut() else {
            return;
        };
        if picker.is_none() && app.picker.is_some() {
            *picker = Some(super::picker::Picker::build(
                driver,
                app.locale,
                &self.emoji,
                &self.composer,
            ));
        }
        let Some(picker) = picker.as_mut() else {
            return;
        };
        picker.sync(app);
        if let Some(tab) = app.picker
            && !picker.popover.is_open()
        {
            picker.show(tab);
        }
    }
}
