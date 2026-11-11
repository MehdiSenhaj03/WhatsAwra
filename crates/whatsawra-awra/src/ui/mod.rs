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

mod call;
mod calls;
mod chat_list;
mod chooser;
mod composer;
mod conversation;
mod dialogs;
mod emoji;
mod group;
mod group_admin;
mod info;
mod lock;
mod login;
mod media;
mod menus;
mod motion;
mod picker;
mod pictures;
mod polls;
mod profile;
mod rows;
mod search;
mod settings;
mod settings_more;
mod shell;
mod viewer;

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use std::time::{Duration, Instant};

use whatsawra::app::App;
use whatsawra::model::Action;

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
    /// When the last turn ended.
    turn_started: Cell<Instant>,
    /// When the turn now queued was asked for: one that starts long after
    /// was kept waiting by the loop (a frame, a layout), which the log says.
    asked: Cell<Option<Instant>>,
    /// What the last ten seconds cost, for the log of a development build.
    stats: RefCell<Stats>,
}

thread_local! {
    static DRIVER: RefCell<Weak<Driver>> = const { RefCell::new(Weak::new()) };
    /// Turns taken, for WHATSAWRA_TURNS (how often the window wakes).
    static TURNS: Cell<u64> = const { Cell::new(0) };
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
    pub waker: Option<whatsawra::backend::Waker>,
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
            turn_started: Cell::new(Instant::now()),
            asked: Cell::new(None),
            stats: RefCell::default(),
        });
        DRIVER.with(|slot| *slot.borrow_mut() = Rc::downgrade(&driver));
        // The app's waker and its timed requests reach Awra's loop from any
        // thread through this callback.
        ctx.set_request_repaint_callback(|info| {
            let delay = info.delay;
            awra::glib::invoke(move || {
                if let Some(driver) = DRIVER.with(|slot| slot.borrow().upgrade()) {
                    driver.wake_for_app(delay);
                }
            });
        });
        driver
    }

    /// A turn the app asked for through its egui context. "At once" means
    /// the next frame there, and code that waits on something asks again
    /// from every frame: taken literally here, where a turn is no frame, it
    /// took hundreds of turns a second. Such a turn starts a frame's time
    /// after the last one at the earliest; what the reader does (`push`)
    /// still gets its turn at once.
    fn wake_for_app(&self, delay: Duration) {
        const FRAME: Duration = Duration::from_millis(8);
        let earliest = FRAME.saturating_sub(self.turn_started.get().elapsed());
        self.wake_in(delay.max(earliest));
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
        self.asked.set(Some(Instant::now()));
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

    /// Carries out what the app asked of the window: quit (the tray's Quit,
    /// Ctrl+Q), hide it (the tray), or show it again (the tray, a
    /// notification, another launch).
    fn follow_window(&self, app: &mut App) {
        let Some(window) = self
            .shell
            .borrow()
            .as_ref()
            .map(|shell| shell.window.clone())
        else {
            return;
        };
        if app.quit_requested {
            awra::quit();
        } else if app.hide_intent {
            app.window_gone();
            window.hide();
        } else if app.wants_show {
            window.present();
            app.frontend_window_shown();
        }
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
        TURNS.with(|turns| turns.set(turns.get() + 1));
        let began = Instant::now();
        // What the reader did waits for this turn: one that starts late is a
        // hitch, whatever the turn itself then costs.
        if let Some(waited) = self.asked.take().map(|asked| asked.elapsed())
            && waited > Duration::from_millis(50)
        {
            let late = format!("a turn waited {} ms for the loop", waited.as_millis());
            if profile::on() {
                eprintln!("{late}");
            }
            timing(&late);
            let mut stats = self.stats.borrow_mut();
            stats.waited = stats.waited.max(waited);
        }
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
            let mut output = profile::span("app", || {
                self.ctx
                    .run_ui(input, |ui| app.frontend_turn(ui.ctx(), focused))
            });
            // Nothing paints the context, so its texture uploads go nowhere.
            output.textures_delta.clear();
            // A picture that changed under its file name is read again.
            pictures::forget(&app.take_stale_pictures());
            // The app brings an open window forward (a second launch, a
            // notification) by asking egui to focus it.
            let raise = output.viewport_output.values().any(|viewport| {
                viewport
                    .commands
                    .iter()
                    .any(|command| matches!(command, egui::ViewportCommand::Focus))
            });
            if raise
                && let Some(window) = self
                    .shell
                    .borrow()
                    .as_ref()
                    .map(|shell| shell.window.clone())
            {
                window.present();
            }
            #[cfg(feature = "demo")]
            if self.demo.get() {
                whatsawra::demo::tour::respond(&mut app);
            }
            self.follow_window(&mut app);
        }
        let synced = Instant::now();
        profile::span("views", || self.sync());
        // A turn that takes a frame or more is what makes the window lag.
        let views = synced.elapsed();
        let took = began.elapsed();
        let whole = self.turn_started.replace(Instant::now()).elapsed();
        {
            let mut stats = self.stats.borrow_mut();
            stats.turns += 1;
            stats.spent += took;
            stats.longest = stats.longest.max(took);
        }
        if took > Duration::from_millis(16) {
            let slow = format!(
                "slow turn: {} ms (views {} ms), {} ms since the last turn",
                took.as_millis(),
                views.as_millis(),
                whole.as_millis()
            );
            if profile::on() {
                eprintln!("{slow}");
            }
            timing(&slow);
        }
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
            // WHATSAWRA_SHOT_SCALE=2 shoots at twice the density, to look closely.
            let scale = std::env::var("WHATSAWRA_SHOT_SCALE")
                .ok()
                .and_then(|scale| scale.parse::<f64>().ok())
                .unwrap_or(1.0);
            awra::Window::offscreen("WhatsAwra", width, height, scale)
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
    driver.app.borrow_mut().frontend_window_shown();
    driver.app.borrow_mut().start_backends();
    // The keyboard starts in the search, not on the window's buttons.
    if let Some(shell) = driver.shell.borrow().as_ref() {
        shell.focus_start();
    }
    match options.shot {
        Some(path) => {
            // Let a few turns settle (pictures, the first page of messages).
            // An offscreen window lays its rows out only when it is drawn,
            // and rows ask for their pictures as they are laid out: drawing
            // while settling is what a window on screen does.
            let start = Instant::now();
            while start.elapsed() < Duration::from_millis(1200) {
                awra::pump();
                window.render_offscreen(false);
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
            // WHATSAWRA_SWITCH=<n>[,<n>…]: opens those chats one after another,
            // two frames apart, as a reader in a hurry does, and shoots every
            // frame, 16 ms apart, with ten more after the last: how a switch
            // looks while it happens.
            if let Ok(list) = std::env::var("WHATSAWRA_SWITCH") {
                let chats: Vec<usize> = list
                    .split(',')
                    .filter_map(|n| n.trim().parse().ok())
                    .collect();
                let stem = path.with_extension("");
                let mut frame = 0;
                for (step, index) in chats.iter().enumerate() {
                    let chat = driver
                        .read(|app| app.visible_chats().get(*index).map(|chat| chat.id.clone()));
                    if let Some(chat) = chat {
                        driver.act(whatsawra::model::Action::OpenChat(chat));
                    }
                    let frames = if step + 1 == chats.len() { 10 } else { 2 };
                    for _ in 0..frames {
                        let started = Instant::now();
                        awra::pump();
                        let pumped = started.elapsed();
                        std::thread::sleep(Duration::from_millis(16));
                        let started = Instant::now();
                        window.render_offscreen(true);
                        eprintln!(
                            "frame {frame} (chat {index}): loop {:.1} ms, drawn in {:.1} ms",
                            pumped.as_secs_f64() * 1000.0,
                            started.elapsed().as_secs_f64() * 1000.0
                        );
                        window.snapshot_png(&format!("{}-f{frame:02}.png", stem.display()));
                        frame += 1;
                    }
                }
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
            // With the tray (or the Dock), closing hides the window; else it quits.
            let weak = Rc::downgrade(&driver);
            window.connect_close_request(move |window| {
                let hides = weak
                    .upgrade()
                    .is_some_and(|driver| driver.app.borrow_mut().frontend_close_hides());
                if hides {
                    window.hide();
                    return awra::glib::Propagation::Stop;
                }
                awra::quit();
                awra::glib::Propagation::Proceed
            });
            watch_timing(&driver);
            if std::env::var_os("WHATSAWRA_BENCH").is_some() {
                bench(&driver);
            }
            if std::env::var_os("WHATSAWRA_TURNS").is_some() {
                // Frames drawn and pixels repainted too: a turn that changes
                // nothing must draw nothing.
                let drawn = Cell::new(window.stats());
                let window = window.clone();
                awra::glib::timeout_add_local(Duration::from_secs(1), move || {
                    let turns = TURNS.with(|turns| turns.replace(0));
                    let now = window.stats();
                    let (frames, pixels) = drawn.replace(now);
                    eprintln!(
                        "turns in the last second: {turns}, frames {}, pixels {}
{}",
                        now.0 - frames,
                        now.1 - pixels,
                        profile::report()
                    );
                    awra::glib::ControlFlow::Continue
                });
            }
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

/// Asks for a turn after `delay` (a clock that moves on its own: a call's
/// time, a recording's). One request stands for all those made before it is
/// due, so a view may ask at every turn without the turns multiplying.
pub(crate) fn wake_after(delay: Duration) {
    if let Some(driver) = DRIVER.with(|slot| slot.borrow().upgrade()) {
        driver.wake_in(delay);
    }
}

/// WHATSAWRA_BENCH: plays the reader for a few seconds (opening chats one
/// after another, typing a line), then prints what each step's turn cost and
/// quits. With `--demo --stress` it measures the window on a busy account.
/// One thing the bench does as the reader.
type Step = Box<dyn Fn(&Rc<Driver>)>;

fn bench(driver: &Rc<Driver>) {
    let chats: Vec<String> = driver.read(|app| {
        app.visible_chats()
            .iter()
            .take(8)
            .map(|chat| chat.id.clone())
            .collect()
    });
    let mut steps: Vec<(&'static str, Step)> = Vec::new();
    for chat in chats {
        steps.push((
            "open a chat",
            Box::new(move |driver: &Rc<Driver>| {
                driver.act(whatsawra::model::Action::OpenChat(chat.clone()))
            }),
        ));
    }
    for length in 1..=8 {
        steps.push((
            "type a letter",
            Box::new(move |driver: &Rc<Driver>| {
                let text = "Hello there, this is a test!!"
                    .chars()
                    .cycle()
                    .take(length)
                    .collect::<String>();
                driver.edit(move |app| {
                    app.composer = text;
                    app.frontend_keystroke();
                })
            }),
        ));
    }
    // How long the loop was held: a timer due every 2 ms that comes late was
    // kept waiting by a turn, a layout or a frame. The longest wait of a step
    // is what the reader feels as a hitch.
    let held: Rc<Cell<(Duration, Duration)>> = Rc::default();
    let last = Cell::new(Instant::now());
    let meter = held.clone();
    awra::glib::timeout_add_local(Duration::from_millis(2), move || {
        let gap = last.replace(Instant::now()).elapsed();
        let (longest, total) = meter.get();
        let over = gap.saturating_sub(Duration::from_millis(4));
        meter.set((longest.max(gap), total + over));
        awra::glib::ControlFlow::Continue
    });
    let weak = Rc::downgrade(driver);
    let at = Cell::new(0usize);
    let _ = profile::report();
    awra::glib::timeout_add_local(Duration::from_millis(400), move || {
        let Some(driver) = weak.upgrade() else {
            return awra::glib::ControlFlow::Break;
        };
        let index = at.get();
        if index > 0 {
            let (name, _) = &steps[index - 1];
            let (longest, total) = held.take();
            eprintln!(
                "after {name}: loop held {:.1} ms at most, {:.1} ms in all\n{}",
                longest.as_secs_f64() * 1000.0,
                total.as_secs_f64() * 1000.0,
                profile::report()
            );
        } else {
            held.take();
        }
        if let Some((_, step)) = steps.get(index) {
            step(&driver);
            at.set(index + 1);
            awra::glib::ControlFlow::Continue
        } else {
            awra::quit();
            awra::glib::ControlFlow::Break
        }
    });
}

/// What a search field and the app last agreed its text was. The field
/// tells the app what was typed only after a pause; a turn taken meanwhile
/// (a message arriving, a picture decoded) must not write the app's older
/// text back over the letters just typed.
#[derive(Clone, Default)]
pub(crate) struct Typed(Rc<RefCell<String>>);

impl Typed {
    /// The field told the app `text`.
    pub fn note(&self, text: &str) {
        *self.0.borrow_mut() = text.to_owned();
    }

    /// Writes the app's `text` into `entry` when the app changed it on its
    /// own (cleared it, restored it), not when the field is merely ahead.
    pub fn follow(&self, entry: &awra::SearchEntry, text: &str) {
        if *self.0.borrow() == text {
            return;
        }
        self.note(text);
        if entry.text() != text {
            entry.set_text(text);
        }
    }
}

/// What the window cost over a stretch of time.
#[derive(Default)]
struct Stats {
    turns: u32,
    /// The time spent in turns, and the longest of them.
    spent: Duration,
    longest: Duration,
    /// The longest a turn waited for the loop after it was asked for.
    waited: Duration,
    /// The longest the loop kept a timer waiting (a layout, a frame).
    held: Duration,
}

/// A line about the window's timing: never anything of a chat. A
/// development build (the one installed to try a change) writes it to the
/// log file, which keeps warnings only; a release keeps it for a debug log.
fn timing(line: &str) {
    if cfg!(debug_assertions) {
        log::warn!("ui timing: {line}");
    } else {
        log::debug!("ui timing: {line}");
    }
}

/// In a development build, a heartbeat that measures how long the loop
/// keeps it waiting, and a line every ten seconds saying what the window
/// cost meanwhile, when it cost anything worth saying.
fn watch_timing(driver: &Rc<Driver>) {
    if !cfg!(debug_assertions) {
        return;
    }
    const BEAT: Duration = Duration::from_millis(100);
    let weak = Rc::downgrade(driver);
    let last = Cell::new(Instant::now());
    let beats = Cell::new(0u32);
    awra::glib::timeout_add_local(BEAT, move || {
        let Some(driver) = weak.upgrade() else {
            return awra::glib::ControlFlow::Break;
        };
        let late = last.replace(Instant::now()).elapsed().saturating_sub(BEAT);
        if late > Duration::from_millis(100) {
            timing(&format!("the loop was held {} ms", late.as_millis()));
        }
        let mut stats = driver.stats.borrow_mut();
        stats.held = stats.held.max(late);
        beats.set(beats.get() + 1);
        if beats.get() >= 100 {
            beats.set(0);
            let stats = std::mem::take(&mut *stats);
            let busy = stats.turns > 300
                || stats.spent > Duration::from_millis(500)
                || stats.longest > Duration::from_millis(30)
                || stats.waited > Duration::from_millis(50)
                || stats.held > Duration::from_millis(50);
            if busy {
                timing(&format!(
                    "10 s: {} turns taking {} ms in all, the longest {} ms; \
                     a turn waited up to {} ms, the loop was held up to {} ms",
                    stats.turns,
                    stats.spent.as_millis(),
                    stats.longest.as_millis(),
                    stats.waited.as_millis(),
                    stats.held.as_millis()
                ));
            }
        }
        awra::glib::ControlFlow::Continue
    });
}
