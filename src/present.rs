//! What the views say about the app, apart from how they draw it: shared by
//! the egui window and the Awra frontend, so both read the same words.

use crate::app::App;
use crate::i18n::gettext;
use crate::model::{Chat, ChatActivity};

/// How a line of status reads: in the accent (someone is doing something
/// now) or quietly.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Accent,
    Secondary,
}

/// The line under a chat's name: who is typing or recording, a group's
/// members, a person's presence or number.
pub fn chat_subtitle(app: &App, chat: &Chat) -> (String, Tone) {
    let typing = app.typing_in(&chat.id);
    if !typing.is_empty() {
        let activity = typing[0].2;
        let text = if chat.is_group() {
            let describe = |activity| {
                let names: Vec<&str> = typing
                    .iter()
                    .filter(|(_, _, other)| *other == activity)
                    .map(|(_, name, _)| name.as_str())
                    .collect();
                match (activity, names.as_slice()) {
                    (_, []) => None,
                    (ChatActivity::RecordingAudio, [one]) => Some(
                        gettext(app.locale, "{name} is recording audio…").replace("{name}", one),
                    ),
                    (ChatActivity::RecordingAudio, [rest @ .., last]) => Some(
                        gettext(app.locale, "{names} and {last} are recording audio…")
                            .replace("{names}", &rest.join(", "))
                            .replace("{last}", last),
                    ),
                    (_, [one]) => Some(format!("{one} is typing…")),
                    (_, [rest @ .., last]) => {
                        Some(format!("{} and {last} are typing…", rest.join(", ")))
                    }
                }
            };
            [ChatActivity::Typing, ChatActivity::RecordingAudio]
                .into_iter()
                .filter_map(describe)
                .collect::<Vec<_>>()
                .join(" · ")
        } else if activity == ChatActivity::RecordingAudio {
            gettext(app.locale, "recording audio…").into_owned()
        } else {
            "typing…".to_owned()
        };
        return (text, Tone::Accent);
    }
    if chat.is_group() {
        let names = app.participant_names(chat);
        return (
            if names.is_empty() {
                "Group".to_owned()
            } else {
                names
            },
            Tone::Secondary,
        );
    }
    if let Some(presence) = app.presence.get(&chat.id) {
        if presence.online {
            return ("online".to_owned(), Tone::Accent);
        }
        if let Some(seen) = presence.last_seen {
            return (crate::util::last_seen(app.locale, seen), Tone::Secondary);
        }
    }
    match chat.phone() {
        Some(phone) if !app.is_saved_contact(&chat.id) => {
            (crate::util::phone(phone), Tone::Secondary)
        }
        _ => (String::new(), Tone::Secondary),
    }
}

/// What a chat row says while someone in it types or records.
pub fn activity_preview(app: &App, chat: &Chat) -> Option<String> {
    let typing = app.typing_in(&chat.id);
    let (_, name, activity) = typing.first()?;
    let doing = match activity {
        ChatActivity::RecordingAudio => gettext(app.locale, "recording audio…").into_owned(),
        ChatActivity::Typing => "typing…".to_owned(),
    };
    Some(if chat.is_group() {
        format!("{name} {doing}")
    } else {
        doing
    })
}
