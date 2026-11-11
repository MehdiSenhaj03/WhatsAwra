//! WhatsAwra drawn with Awra: the same client as `whatsawra`, its window
//! made of Awra's components.
//!
//! `--demo` shows the synthetic offline sample (with the `demo` feature);
//! `--shot <png>` renders the window offscreen to a picture and quits;
//! `--size WxH` sets the shot's size; `--open <n>` opens the n-th chat.
#![windows_subsystem = "windows"]

use std::path::PathBuf;

use whatsawra::{app, awra_ui, paths, settings};

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

    let waker = whatsawra::backend::Waker::default();
    #[allow(unused_mut)]
    let mut app = if demo {
        app::App::headless(dirs, settings).0
    } else {
        match app::App::new(&waker, dirs, settings, app::AppOptions { tray: false }) {
            Ok(app) => app,
            Err(error) => {
                eprintln!("WhatsAwra could not start: {error}");
                std::process::exit(1);
            }
        }
    };
    #[cfg(feature = "demo")]
    if demo {
        whatsawra::demo::populate(&mut app);
    }
    if let Some(index) = arg_value(&args, "--open").and_then(|n| n.parse::<usize>().ok())
        && let Some(chat) = app.visible_chats().get(index).map(|chat| chat.id.clone())
    {
        app.queue(whatsawra::model::Action::OpenChat(chat));
    }
    awra_ui::run(
        app,
        awra_ui::Options {
            shot,
            size,
            waker: Some(waker),
        },
    );
    if let Some(root) = demo_root {
        let _ = std::fs::remove_dir_all(root);
    }
}
