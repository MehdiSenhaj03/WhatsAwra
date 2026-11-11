//! What each confirmation dialog asks and what its buttons do, apart from
//! how a window draws it: the egui window and the Awra frontend show the
//! same question, consequences and choices.

use crate::app::App;
use crate::i18n::{gettext, ngettext};
use crate::model::{Action, Dialog};

/// A question before something that cannot be taken back.
#[derive(Clone, Debug)]
pub struct Confirmation {
    pub heading: String,
    /// The consequences: the first paragraph in the text colour, any after
    /// it as a quieter note.
    pub body: Vec<String>,
    /// The ways to go ahead, the main one first; Cancel is always offered.
    pub choices: Vec<Choice>,
    /// Whether the choices can be taken now (blocking needs the link).
    pub enabled: bool,
}

#[derive(Clone, Debug)]
pub struct Choice {
    pub label: String,
    /// Queued when chosen, followed by `Action::CloseDialog` when `closes`.
    pub actions: Vec<Action>,
    /// Drawn as the destructive choice.
    pub destructive: bool,
    pub closes: bool,
}

impl Choice {
    fn destructive(label: impl Into<String>, actions: Vec<Action>) -> Self {
        Self {
            label: label.into(),
            actions,
            destructive: true,
            closes: true,
        }
    }
}

impl Confirmation {
    fn new(heading: impl Into<String>, body: Vec<String>, choice: Choice) -> Self {
        Self {
            heading: heading.into(),
            body,
            choices: vec![choice],
            enabled: true,
        }
    }
}

/// The confirmation `dialog` is, if it is one.
pub fn of(app: &App, dialog: &Dialog) -> Option<Confirmation> {
    let locale = app.locale;
    let t = |text: &'static str| gettext(locale, text).into_owned();
    let chat_name = |id: &str| {
        app.chat(id)
            .map_or_else(|| t("this chat"), |chat| chat.name.clone())
    };
    Some(match dialog {
        Dialog::ConfirmDeleteChat(id) => Confirmation::new(
            t("Delete chat?"),
            vec![t("This deletes your chat with {name}, including all messages, on this computer and on your phone. It cannot be undone.").replace("{name}", &chat_name(id))],
            Choice::destructive(t("Delete"), vec![Action::DeleteChat(id.clone())]),
        ),
        Dialog::ConfirmClearChat(id) => Confirmation::new(
            t("Clear chat?"),
            vec![t("This clears every message in your chat with {name}, including downloaded media, on this computer and on your phone. The chat itself stays. It cannot be undone.").replace("{name}", &chat_name(id))],
            Choice::destructive(t("Clear chat"), vec![Action::ClearChat(id.clone())]),
        ),
        Dialog::ConfirmStartOver => Confirmation::new(
            t("Start over?"),
            vec![
                t("WhatsAwra keeps your unreadable archive as a separate file, creates a new one, and asks you to link again. Linking again brings back recent history from your phone. Afterwards, remove the old WhatsAwra entry under Linked devices on your phone."),
                t("If you can restore the original keyring instead, choose Cancel and Try again: nothing is lost that way."),
            ],
            Choice::destructive(t("Start over"), vec![Action::StartOverArchive]),
        ),
        Dialog::ConfirmDeleteMessage {
            chat,
            message,
            for_everyone,
        } => delete_messages(app, chat, std::slice::from_ref(message), *for_everyone, false),
        Dialog::ConfirmDeleteSelection {
            chat,
            messages,
            for_everyone,
        } => delete_messages(app, chat, messages, *for_everyone, true),
        Dialog::ConfirmRemoveStickerPack { name, dir } => Confirmation::new(
            t("Remove sticker pack?"),
            vec![t("This deletes \"{name}\" and its stickers from this computer. To use it again, import it again.").replace("{name}", name)],
            Choice::destructive(t("Remove"), vec![Action::DeleteStickerPack(dir.clone())]),
        ),
        Dialog::ConfirmUnlink => Confirmation::new(
            t("Unlink this computer?"),
            vec![t("This removes the device from WhatsApp and deletes the chats stored here. You can link again with a new code.")],
            Choice::destructive(t("Unlink"), vec![Action::Unlink]),
        ),
        Dialog::ConfirmClearMediaCache(id) => {
            let account = app
                .accounts
                .iter()
                .find(|account| &account.id == id)
                .map(|account| account.display_label(locale))
                .unwrap_or_default();
            Confirmation::new(
                t("Clear the media cache?"),
                vec![
                    t("This deletes the photos, videos, stickers and files {account} downloaded to this computer. Messages stay, and attachments download again when you open them, while WhatsApp still has them.").replace("{account}", &account),
                    t("Files in a download folder you chose are kept."),
                ],
                Choice::destructive(t("Clear"), vec![Action::ClearMediaCache(id.clone())]),
            )
        }
        Dialog::ConfirmRemoveAccount(id) => Confirmation::new(
            t("Remove this account?"),
            vec![t("This unlinks the number on this computer and deletes its local chats. Other accounts stay.")],
            Choice::destructive(t("Remove"), vec![Action::RemoveAccount(id.clone())]),
        ),
        Dialog::ConfirmBlock { chat, blocked, .. } => {
            // A locked chat keeps its name behind the lock.
            let name = if app.chat(chat).is_some_and(|chat| chat.locked) && !app.locked_folder_open() {
                t("Locked chat")
            } else {
                app.chat_title_of(chat)
            };
            let (heading, body, button) = if *blocked {
                (
                    t("Block contact?"),
                    t("Block {name}? You will no longer receive their direct messages or calls. This keeps your chat history."),
                    t("Block"),
                )
            } else {
                (
                    t("Unblock contact?"),
                    t("Unblock {name}? You will be able to exchange messages and calls again."),
                    t("Unblock"),
                )
            };
            Confirmation {
                enabled: app.is_connected() && app.blocklist.editable(),
                ..Confirmation::new(
                    heading,
                    vec![body.replace("{name}", &name)],
                    Choice::destructive(
                        button,
                        vec![Action::SetBlocked {
                            chat: chat.clone(),
                            blocked: *blocked,
                        }],
                    ),
                )
            }
        }
        Dialog::ConfirmLockChat(id) => {
            let mut actions = vec![Action::SetLocked(id.clone(), true)];
            if app.settings.chat_lock_code_hash.is_none() {
                actions.push(Action::OpenLockedFolder);
            }
            Confirmation::new(
                t("Lock this chat?"),
                vec![t("This moves the chat to the Locked tab, hiding it from the normal chat list, search, unread badges, and notifications. The lock status also syncs to your phone. Open Locked with a separate local WhatsAwra code to read it.")],
                Choice {
                    destructive: false,
                    ..Choice::destructive(t("Lock chat"), actions)
                },
            )
        }
        Dialog::ConfirmLeaveGroup(id) => {
            let chat = app.chat(id);
            let archived = chat.is_some_and(|chat| chat.archived);
            let channel = chat.is_some_and(crate::model::Chat::is_channel);
            let leave = |archive| Action::LeaveGroup {
                chat: id.clone(),
                archive,
            };
            let mut confirmation = Confirmation::new(
                if channel {
                    t("Leave this channel?")
                } else {
                    t("Leave this group?")
                },
                vec![t("You will not receive new messages. The local history stays on this computer.")],
                Choice {
                    closes: false,
                    ..Choice::destructive(
                        if channel {
                            t("Leave channel")
                        } else {
                            t("Leave group")
                        },
                        vec![leave(false)],
                    )
                },
            );
            if !archived {
                confirmation.choices.push(Choice {
                    label: if channel {
                        t("Leave channel and archive")
                    } else {
                        t("Leave group and archive")
                    },
                    actions: vec![leave(true)],
                    destructive: false,
                    closes: false,
                });
            }
            confirmation
        }
        Dialog::ConfirmResetInviteLink(chat) => Confirmation::new(
            t("Reset the invite link?"),
            vec![t("The current link will stop working, and nobody can join with it any more. A new link replaces it.")],
            Choice {
                closes: false,
                ..Choice::destructive(t("Reset link"), vec![Action::ResetInviteLink(chat.clone())])
            },
        ),
        Dialog::ConfirmRemoveMember { chat, member } => {
            let group = app
                .chat(chat)
                .map(|chat| app.chat_title(chat))
                .unwrap_or_default();
            Confirmation::new(
                t("Remove from the group?"),
                vec![t("{name} will no longer receive messages from {group}. You can add them again later.")
                    .replace("{name}", &app.display_name(member))
                    .replace("{group}", &group)],
                Choice {
                    closes: false,
                    ..Choice::destructive(
                        t("Remove"),
                        vec![Action::ChangeGroupMember {
                            chat: chat.clone(),
                            member: member.clone(),
                            change: crate::group_admin::MemberChange::Remove,
                        }],
                    )
                },
            )
        }
        _ => return None,
    })
}

/// Deleting one message, or the whole selection, for us or for everyone.
fn delete_messages(
    app: &App,
    chat: &str,
    ids: &[String],
    for_everyone: bool,
    selection: bool,
) -> Confirmation {
    let locale = app.locale;
    let heading = if for_everyone {
        gettext(locale, "Delete for everyone?")
    } else {
        gettext(locale, "Delete for me?")
    };
    let body = deletion_body(locale, for_everyone, selection.then_some(ids.len()));
    let mut paragraphs = Vec::new();
    if selection && !ids.is_empty() {
        paragraphs.push(selected_message_count(locale, ids.len()));
    }
    paragraphs.push(body);
    let mut actions: Vec<Action> = ids
        .iter()
        .map(|id| {
            let (chat, id) = (chat.to_owned(), id.clone());
            if for_everyone {
                Action::DeleteForEveryone { chat, id }
            } else {
                Action::DeleteForMe { chat, id }
            }
        })
        .collect();
    if selection {
        actions.push(Action::CancelSelection);
    }
    Confirmation::new(
        heading.into_owned(),
        paragraphs,
        Choice::destructive(gettext(locale, "Delete"), actions),
    )
}

/// What deleting does, for one message or a selection, for us or everyone.
pub fn deletion_body(
    locale: crate::i18n::Locale,
    for_everyone: bool,
    count: Option<usize>,
) -> String {
    match (for_everyone, count) {
        (false, None) => gettext(locale, "This removes the message from your phone and linked devices. Other people keep their copy. Connect to WhatsApp to delete it. This cannot be undone.").into_owned(),
        (true, None) => gettext(locale, "Everyone in this chat will see \"This message was deleted\" instead. It cannot be undone.").into_owned(),
        (false, Some(_)) => gettext(locale, "This removes these messages from your phone and linked devices. Other people keep their copy. Connect to WhatsApp to delete them. This cannot be undone.").into_owned(),
        (true, Some(count)) => ngettext(locale, "Everyone in this chat will see \"This message was deleted\" in place of the selected message. This cannot be undone.", "Everyone in this chat will see \"This message was deleted\" in place of the selected messages. This cannot be undone.", count as u32).into_owned(),
    }
}

/// "3 selected messages".
pub fn selected_message_count(locale: crate::i18n::Locale, count: usize) -> String {
    ngettext(
        locale,
        "{count} selected message",
        "{count} selected messages",
        count as u32,
    )
    .replace("{count}", &count.to_string())
}
