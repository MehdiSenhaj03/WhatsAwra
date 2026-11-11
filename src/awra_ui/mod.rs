//! WhatsAwra drawn with Awra.
//!
//! The app (`App`: accounts, backends, archive, every rule of the client)
//! is the same one the egui window drives. Here it runs in turns on Awra's
//! main loop: a turn applies backend events and queued actions
//! (`App::frontend_turn`), then the views read the app and update their
//! widgets. Widgets never change the app themselves: their signals queue an
//! [`Op`] and ask for a turn, as egui's views push `Action`s.
//!
//! The app still asks for turns through an egui context (its `Waker` and
//! `request_repaint_after`), which here has no window: its repaint callback
//! wakes Awra's loop instead.

mod calls;
mod chat_list;
mod conversation;
mod dialogs;
mod info;
mod login;
mod media;
mod menus;
mod pictures;
mod settings;
mod shell;

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use std::time::{Duration, Instant};

use crate::app::App;
use crate::model::Action;

/// A change a view asks of the app.
pub enum Op {
    /// What an egui view would push to `App::actions`.
    Action(Action),
    /// A view's own field in the app (composer text, search text).
    Edit(Box<dyn FnOnce(&mut App)>),
}

/// The app and the window that shows it.
pub struct Driver {
    app: RefCell<App>,
    ctx: egui::Context,
    started: Instant,
    ops: RefCell<Vec<Op>>,
    /// A turn is already queued on the main loop.
    queued: Cell<bool>,
    /// When the app asked to be woken at the latest (`request_repaint_after`).
    alarm: Cell<Option<Instant>>,
    shell: RefCell<Option<shell::Shell>>,
    /// Set while the views write to their widgets, whose signals would
    /// otherwise queue the very values the views just wrote.
    syncing: Cell<bool>,
    demo: Cell<bool>,
}

thread_local! {
    static DRIVER: RefCell<Weak<Driver>> = RefCell::new(Weak::new());
}

/// Options of a run.
#[derive(Default)]
pub struct Options {
    /// Render the window offscreen to this PNG after the app settles, then
    /// quit (`--shot`).
    pub shot: Option<std::path::PathBuf>,
    /// Window size for a shot.
    pub size: Option<(i32, i32)>,
    /// The waker the app's backends were made with, which wakes this loop.
    pub waker: Option<crate::backend::Waker>,
    /// The offline sample: the commands the views send are answered here,
    /// as the egui demo answers them.
    pub demo: bool,
    /// For a shot: open a menu first (`message`: the newest message's, `chat`:
    /// the first chat's).
    pub menu: Option<String>,
}

impl Driver {
    fn new(app: App) -> Rc<Self> {
        let ctx = egui::Context::default();
        let driver = Rc::new(Self {
            app: RefCell::new(app),
            ctx: ctx.clone(),
            started: Instant::now(),
            ops: RefCell::new(Vec::new()),
            queued: Cell::new(false),
            alarm: Cell::new(None),
            shell: RefCell::new(None),
            syncing: Cell::new(false),
            demo: Cell::new(false),
        });
        DRIVER.with(|slot| *slot.borrow_mut() = Rc::downgrade(&driver));
        // The app's waker and its timed requests reach Awra's loop from any
        // thread through this callback.
        ctx.set_request_repaint_callback(|info| {
            let delay = info.delay;
            awra::glib::invoke(move || {
                if let Some(driver) = DRIVER.with(|slot| slot.borrow().upgrade()) {
                    driver.wake_in(delay);
                }
            });
        });
        driver
    }

    /// Queues `op` and asks for a turn. Ignored while the views write their
    /// widgets.
    pub fn push(&self, op: Op) {
        if self.syncing.get() {
            return;
        }
        self.ops.borrow_mut().push(op);
        self.wake();
    }

    /// Queues `action` for the next turn.
    pub fn act(&self, action: Action) {
        self.push(Op::Action(action));
    }

    /// Changes a view's own field of the app on the next turn.
    pub fn edit(&self, edit: impl FnOnce(&mut App) + 'static) {
        self.push(Op::Edit(Box::new(edit)));
    }

    /// Queues `op` from a view's sync, which needs something the app does
    /// not have yet (a picture to fetch, a page of messages to load). Unlike
    /// [`push`](Self::push) it is kept while the views sync; the view must
    /// ask only once for each thing, or every turn would ask for the next.
    pub fn later(&self, op: Op) {
        self.ops.borrow_mut().push(op);
        self.wake();
    }

    /// Reads the app outside a turn (a signal handler choosing what to do).
    pub fn read<R>(&self, read: impl FnOnce(&App) -> R) -> R {
        read(&self.app.borrow())
    }

    /// Asks for a turn as soon as the loop is idle.
    pub fn wake(&self) {
        if self.queued.replace(true) {
            return;
        }
        awra::glib::idle_add_local_once(|| {
            if let Some(driver) = DRIVER.with(|slot| slot.borrow().upgrade()) {
                driver.queued.set(false);
                driver.turn();
            }
        });
    }

    /// Asks for a turn after `delay`, unless one is due sooner.
    fn wake_in(&self, delay: Duration) {
        if delay.is_zero() {
            self.wake();
            return;
        }
        // egui says "never" with a huge delay.
        if delay > Duration::from_secs(24 * 3600) {
            return;
        }
        let at = Instant::now() + delay;
        if self.alarm.get().is_some_and(|due| due <= at) {
            return;
        }
        self.alarm.set(Some(at));
        awra::glib::timeout_add_local_once(delay, move || {
            if let Some(driver) = DRIVER.with(|slot| slot.borrow().upgrade()) {
                if driver.alarm.get() == Some(at) {
                    driver.alarm.set(None);
                }
                driver.wake();
            }
        });
    }

    fn focused(&self) -> bool {
        self.shell
            .borrow()
            .as_ref()
            .is_none_or(|shell| shell.window.is_active())
    }

    /// One turn: the views' ops, the app's events and actions, then the
    /// views read the result.
    fn turn(self: &Rc<Self>) {
        let focused = self.focused();
        {
            let mut app = self.app.borrow_mut();
            for op in std::mem::take(&mut *self.ops.borrow_mut()) {
                match op {
                    Op::Action(action) => app.queue(action),
                    Op::Edit(edit) => edit(&mut app),
                }
            }
            let input = egui::RawInput {
                time: Some(self.started.elapsed().as_secs_f64()),
                focused,
                ..Default::default()
            };
            // An empty pass keeps the context's clock and repaint requests
            // moving, as a window's frames would.
            let mut output = self
                .ctx
                .run_ui(input, |ui| app.frontend_turn(ui.ctx(), focused));
            // Nothing paints the context, so its texture uploads go nowhere.
            output.textures_delta.clear();
            #[cfg(feature = "demo")]
            if self.demo.get() {
                crate::demo::tour::respond(&mut app);
            }
        }
        self.sync();
    }

    /// Lets the views read the app and update their widgets.
    fn sync(self: &Rc<Self>) {
        let app = self.app.borrow();
        self.syncing.set(true);
        // The views are made in the interface language: a new one remakes them.
        let relabel = self
            .shell
            .borrow()
            .as_ref()
            .is_some_and(|shell| shell.locale != app.locale);
        if relabel {
            let window = self
                .shell
                .borrow()
                .as_ref()
                .map(|shell| shell.window.clone());
            if let Some(window) = window {
                drop(app);
                let shell = shell::Shell::build(self, window);
                *self.shell.borrow_mut() = Some(shell);
                self.syncing.set(false);
                return self.sync();
            }
        }
        if let Some(shell) = self.shell.borrow_mut().as_mut() {
            shell.sync(self, &app);
        }
        self.syncing.set(false);
    }
}

/// Runs WhatsAwra's window on Awra until it closes.
pub fn run(app: App, options: Options) {
    let application = awra::Application::new("rocks.whatsawra.WhatsAwra");
    let driver = Driver::new(app);
    driver.demo.set(options.demo);
    if let Some(waker) = &options.waker {
        waker.attach(&driver.ctx);
    }
    let window = match &options.shot {
        Some(_) => {
            let (width, height) = options.size.unwrap_or((1280, 820));
            awra::Window::offscreen("WhatsAwra", width, height, 1.0)
        }
        None => {
            let window = awra::Window::new(&application);
            window.set_title(Some("WhatsAwra"));
            window.set_default_size(1180, 760);
            window
        }
    };
    let shell = shell::Shell::build(&driver, window.clone());
    *driver.shell.borrow_mut() = Some(shell);
    driver.turn();
    window.present();
    driver.app.borrow_mut().start_backends();
    match options.shot {
        Some(path) => {
            // Let a few turns settle (pictures, the first page of messages).
            let start = Instant::now();
            while start.elapsed() < Duration::from_millis(1200) {
                awra::pump();
                std::thread::sleep(Duration::from_millis(15));
            }
            driver.turn();
            if let Some(menu) = options.menu.as_deref()
                && let Some(shell) = driver.shell.borrow().as_ref()
            {
                shell.open_menu(&driver, menu);
            }
            for _ in 0..30 {
                awra::pump();
                std::thread::sleep(Duration::from_millis(10));
            }
            // Dialogs and other windows opened for the shot, beside it.
            let stem = path.with_extension("");
            for (index, other) in awra::Window::toplevels()
                .iter()
                .filter(|other| **other != window)
                .enumerate()
            {
                other.render_offscreen(true);
                other.snapshot_png(&format!("{}-win{index}.png", stem.display()));
            }
            let drawn = window.render_offscreen(true);
            if std::env::var_os("WHATSAWRA_SHOT_DEBUG").is_some() {
                let (min, natural, _, _) = window.measure(awra::Orientation::Horizontal, -1);
                eprintln!(
                    "render {drawn}, window {}x{}, width min {min} natural {natural}",
                    window.width(),
                    window.height()
                );
            }
            if !window.snapshot_png(&path.to_string_lossy()) {
                log::error!("could not write {}", path.display());
            }
        }
        None => {
            window.connect_close_request(|_| {
                awra::quit();
                awra::glib::Propagation::Proceed
            });
            awra::run();
        }
    }
    driver.app.borrow_mut().shutdown_for_frontend();
    drop(application);
}

/// Asks for a turn from anywhere on the UI thread (a picture finished
/// decoding), once the driver exists.
pub(crate) fn wake() {
    if let Some(driver) = DRIVER.with(|slot| slot.borrow().upgrade()) {
        driver.wake();
    }
}
