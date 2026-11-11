//! WhatsAwra drawn with Awra: the same client as `whatsawra`, its window
//! made of Awra's components.
//!
//! `--demo` shows the synthetic offline sample (with the `demo` feature);
//! `--shot <png>` renders the window offscreen to a picture and quits;
//! `--size WxH` sets the shot's size; `--open <n>` opens the n-th chat;
//! `--page <flags>` applies the demo's sample states (as `whatsawra --demo-page`).
//! `--place settings` or `--place calls` opens that page; `--poll` the open
//! chat's Create poll; `--info` the open
//! chat's info.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;

mod ui;

use whatsawra::{app, paths, settings};

fn arg_value(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|arg| arg == name)
        .and_then(|at| args.get(at + 1).cloned())
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let demo = args.iter().any(|arg| arg == "--demo");
    let shot = arg_value(&args, "--shot").map(PathBuf::from);
    let size = arg_value(&args, "--size").and_then(|value| {
        let (width, height) = value.split_once('x')?;
        Some((width.parse().ok()?, height.parse().ok()?))
    });
    if args.iter().any(|arg| arg == "--dark") {
        // SAFETY: before any other thread starts.
        unsafe { std::env::set_var("AWRA_APPEARANCE", "dark") };
    } else if args.iter().any(|arg| arg == "--light") {
        // SAFETY: before any other thread starts.
        unsafe { std::env::set_var("AWRA_APPEARANCE", "light") };
    }

    // A demo keeps its files in a throwaway folder, never the real one.
    let demo_root = demo
        .then(|| std::env::temp_dir().join(format!("whatsawra-awra-demo-{}", std::process::id())));
    let dirs = match &demo_root {
        Some(root) => paths::AppDirs::under(root),
        None => paths::AppDirs::discover(),
    };
    let waker = whatsawra::backend::Waker::default();
    // One copy of WhatsAwra at a time owns the account's session and
    // archive, whichever window it draws: a second launch asks the running
    // one to show itself. Demo runs keep to their own folder.
    let instance = if demo {
        None
    } else {
        use whatsawra::single_instance::{Outcome, acquire};
        match acquire(&dirs.runtime, &waker, "show") {
            Outcome::Only(guard) => Some(guard),
            Outcome::Surfaced => {
                eprintln!("WhatsAwra is already running; asked it to show its window");
                return;
            }
            Outcome::Outdated | Outcome::Unanswered => {
                eprintln!("WhatsAwra is already running");
                return;
            }
        }
    };
    if let Err(error) = dirs.ensure() {
        eprintln!("WhatsAwra cannot create its folders: {error}");
        std::process::exit(1);
    }
    let logging = fastframe_log::Logging::new("whatsawra", env!("CARGO_PKG_VERSION"));
    let logging = if demo {
        logging
    } else {
        logging.file(dirs.log_file())
    };
    let _ = logging.init();
    if !demo && let Err(error) = dirs.adopt_single_account() {
        log::error!("could not move the linked account into its folder: {error}");
        std::process::exit(1);
    }
    let settings = settings::Settings::load(&dirs.settings_file());

    #[allow(unused_mut)]
    let mut app = if demo {
        app::App::headless(dirs, settings).0
    } else {
        match app::App::new(&waker, dirs, settings, app::AppOptions { tray: !demo }) {
            Ok(app) => app,
            Err(error) => {
                eprintln!("WhatsAwra could not start: {error}");
                std::process::exit(1);
            }
        }
    };
    if let Some(guard) = &instance {
        app.set_remote_control(guard);
    }
    #[cfg(feature = "demo")]
    if demo {
        whatsawra::demo::populate(&mut app);
        whatsawra::demo::apply_flags(&mut app, arg_value(&args, "--page").as_deref());
        if args.iter().any(|arg| arg == "--stress") {
            stress(&mut app);
        }
    }
    if let Some(index) = arg_value(&args, "--open").and_then(|n| n.parse::<usize>().ok())
        && let Some(chat) = app.visible_chats().get(index).map(|chat| chat.id.clone())
    {
        app.queue(whatsawra::model::Action::OpenChat(chat.clone()));
        if args.iter().any(|arg| arg == "--poll") {
            app.queue(whatsawra::model::Action::ShowDialog(
                whatsawra::model::Dialog::CreatePoll(chat.clone()),
            ));
        }
        if args.iter().any(|arg| arg == "--info") {
            app.queue(whatsawra::model::Action::OpenInfo(chat));
        }
    }
    match arg_value(&args, "--place").as_deref() {
        Some("settings") => app.queue(whatsawra::model::Action::Open(
            whatsawra::model::Page::Settings,
        )),
        Some("calls") => app.queue(whatsawra::model::Action::Open(
            whatsawra::model::Page::Calls,
        )),
        _ => {}
    }
    ui::run(
        app,
        ui::Options {
            shot,
            size,
            waker: Some(waker),
            menu: arg_value(&args, "--menu"),
            demo,
        },
    );
    drop(instance);
    if let Some(root) = demo_root {
        let _ = std::fs::remove_dir_all(root);
    }
}

/// `--stress`: the sample made as large as a busy account, to measure the
/// window with: four hundred chats, and six hundred more messages in each
/// of the sample's own.
#[cfg(feature = "demo")]
fn stress(app: &mut app::App) {
    let samples = app.chats.clone();
    for index in 0..400 {
        let mut chat = samples[index % samples.len()].clone();
        chat.id = format!("1555{index:07}@s.whatsapp.net");
        chat.name = format!("{} {index}", chat.name);
        chat.pinned = false;
        chat.last_activity -= 60 * (index as i64 + 1);
        app.chats.push(chat);
    }
    // Every sample chat becomes a long one, its own messages last: a switch
    // between two of them is a switch between two busy chats.
    let ids: Vec<String> = samples.iter().map(|chat| chat.id.clone()).collect();
    for id in ids {
        let conversation = app.conversations.entry(id).or_default();
        let Some(template) = conversation
            .messages
            .iter()
            .find(|message| matches!(message.content, whatsawra::model::Content::Text { .. }))
            .cloned()
        else {
            continue;
        };
        let senders: Vec<(String, Option<String>, bool)> = conversation
            .messages
            .iter()
            .map(|message| {
                (
                    message.sender.clone(),
                    message.sender_name.clone(),
                    message.from_me,
                )
            })
            .collect();
        let oldest = conversation
            .messages
            .first()
            .map_or(template.timestamp, |message| message.timestamp);
        let mut older = Vec::new();
        for index in 0..600usize {
            let mut message = template.clone();
            let (sender, name, from_me) = senders[(index / 3) % senders.len()].clone();
            message.id = format!("stress-{index}");
            message.sender = sender;
            message.sender_name = name;
            message.from_me = from_me;
            message.quoted = None;
            message.reactions.clear();
            message.timestamp = oldest - (600 - index as i64) * 500;
            let text = match index % 5 {
                0 => format!("Message {index}"),
                1 => format!(
                    "Message {index}: *bold* and _italic_ with a link https://example.com/{index} and an emoji 😀"
                ),
                2 => format!(
                    "Message {index}, a longer one that wraps over a few lines in its bubble: \
                     the quick brown fox jumps over the lazy dog, again and again, until \
                     the line is long enough to need three of them. 🦊🐶"
                ),
                3 => "👍".to_owned(),
                _ => format!("ok {index}\nsecond line"),
            };
            message.content = whatsawra::model::Content::Text {
                text,
                preview: None,
            };
            older.push(message);
        }
        older.append(&mut conversation.messages);
        conversation.messages = older;
    }
}
