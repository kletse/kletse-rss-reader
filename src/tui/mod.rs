//! Kletse RSS in a terminal, filling it (the alternate screen, so your shell is back as it was afterwards). Works
//! wherever a terminal does: Linux and macOS, locally and over SSH, without Qt.
//!
//! It's a third face on `App`, alongside bridge.rs and ffi.rs: this file runs the terminal and the effects, ui.rs
//! keeps what the terminal needs around the app, draw.rs draws it and article.rs lays out articles.
//!
//! Everything happens on the main thread, which waits on one channel: a thread reads the terminal into it, and the
//! tokio runtime sends the answers to effects into it. After each batch it draws, and ratatui only sends the cells
//! that changed.

mod article;
mod draw;
mod editor;
mod images;
#[cfg(test)]
mod tests;
mod ui;

use crate::app::{Effect, Outcome};
use crate::backend::Backend;
use crate::prefs;
use images::{Answer, Job};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::crossterm::event::{self, DisableBracketedPaste, EnableBracketedPaste, Event as TermEvent};
use ratatui::crossterm::execute;
use ratatui::crossterm::style::Print;
use ratatui::crossterm::terminal::{
    BeginSynchronizedUpdate, Clear, ClearType, EndSynchronizedUpdate, EnterAlternateScreen, LeaveAlternateScreen,
    SetTitle, disable_raw_mode, enable_raw_mode,
};
use ratatui_image::picker::Picker;
use std::io::{self, IsTerminal, Stdout, stdout};
use std::sync::{Arc, mpsc};
use ui::Ui;

/// Press, release and the wheel, as SGR: not every movement of the mouse, as crossterm's own mouse capture asks
/// for. Without it, terminals turn the wheel into ↑ and ↓ on this screen, and ↓ marks articles read.
const MOUSE_ON: &str = "\x1b[?1000h\x1b[?1006h";
const MOUSE_OFF: &str = "\x1b[?1006l\x1b[?1000l";
/// Keeps the title the terminal had, to put back.
const SAVE_TITLE: &str = "\x1b[22;0t";
const RESTORE_TITLE: &str = "\x1b[23;0t";

enum Event {
    Terminal(TermEvent),
    /// The terminal is gone.
    Closed,
    Outcome(Outcome),
    Image(Answer),
}

fn leave_terminal() {
    // Each on its own: one failing is no reason to leave the terminal in a worse state.
    let _ = disable_raw_mode();
    let _ = execute!(
        stdout(),
        Print(MOUSE_OFF),
        DisableBracketedPaste,
        Print(RESTORE_TITLE),
        LeaveAlternateScreen,
        ratatui::crossterm::cursor::Show
    );
}

/// The terminal as we want it, for as long as this lives.
struct Screen;

impl Screen {
    fn enter() -> io::Result<Self> {
        enable_raw_mode()?;
        let screen = Self;
        execute!(stdout(), EnterAlternateScreen, EnableBracketedPaste, Print(MOUSE_ON), Print(SAVE_TITLE))?;
        let hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            leave_terminal();
            hook(info);
        }));
        Ok(screen)
    }
}

impl Drop for Screen {
    fn drop(&mut self) {
        leave_terminal();
    }
}

/// What the terminal can draw. Under tmux (also over SSH from one, which only `TERM` tells) the query would change
/// the pane's `allow-passthrough` and, if nothing outside answers, leave a thread behind that reads your keys.
/// Half-blocks work there as they are. Everywhere else the query ends in a status report every terminal answers.
fn graphics() -> Picker {
    let multiplexed = |term: &str| term.starts_with("tmux") || term.starts_with("screen");
    if std::env::var_os("TMUX").is_some() || std::env::var("TERM").is_ok_and(|t| multiplexed(&t)) {
        return Picker::halfblocks();
    }
    Picker::from_query_stdio().unwrap_or_else(|_| Picker::halfblocks())
}

fn read_terminal(events: mpsc::Sender<Event>) {
    std::thread::spawn(move || {
        while let Ok(event) = event::read() {
            if events.send(Event::Terminal(event)).is_err() {
                return;
            }
        }
        let _ = events.send(Event::Closed);
    });
}

fn handle(ui: &mut Ui, event: Event) {
    match event {
        Event::Terminal(TermEvent::Key(key)) => ui.key(key),
        Event::Terminal(TermEvent::Mouse(mouse)) => ui.mouse(mouse),
        Event::Terminal(TermEvent::Paste(text)) => ui.paste(&text),
        // A resize, focus: the next draw deals with it.
        Event::Terminal(_) => {}
        Event::Closed => ui.quit(),
        Event::Outcome(outcome) => ui.outcome(outcome),
        Event::Image(answer) => ui.image(answer),
    }
}

struct Runner {
    runtime: tokio::runtime::Runtime,
    backend: Arc<Backend>,
    picker: Picker,
    events: mpsc::Sender<Event>,
}

impl Runner {
    fn effect(&self, effect: Effect) {
        let (backend, events) = (Arc::clone(&self.backend), self.events.clone());
        self.runtime.spawn(async move {
            if let Some(outcome) = backend.run(effect).await {
                let _ = events.send(Event::Outcome(outcome));
            }
        });
    }

    fn job(&self, job: Job) {
        let (backend, picker, events) = (Arc::clone(&self.backend), self.picker.clone(), self.events.clone());
        self.runtime.spawn(async move {
            let _ = events.send(Event::Image(images::run(job, backend, picker).await));
        });
    }
}

/// Paints the screen from scratch at the next draw. Not `Terminal::clear`: that asks the terminal for the cursor,
/// and the answer can't get past the thread waiting in `event::read`, so it stalls for 2 s and fails. Swapping
/// once more than a draw does leaves both of ratatui's buffers blank, like the screen.
fn repaint(terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> io::Result<()> {
    execute!(terminal.backend_mut(), Clear(ClearType::All))?;
    terminal.swap_buffers();
    Ok(())
}

fn event_loop(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    ui: &mut Ui,
    runner: &Runner,
    inbox: &mpsc::Receiver<Event>,
) -> io::Result<()> {
    let mut title = String::new();
    loop {
        ui.prepare(terminal.size()?.into());
        // Where it's supported, this keeps a repaint from showing as a flash; elsewhere it does nothing.
        execute!(terminal.backend_mut(), BeginSynchronizedUpdate)?;
        if ui.take_repaint() {
            repaint(terminal)?;
        }
        terminal.draw(|frame| draw::draw(ui, frame))?;
        execute!(terminal.backend_mut(), EndSynchronizedUpdate)?;
        ui.take_effects().into_iter().for_each(|effect| runner.effect(effect));
        ui.take_jobs().into_iter().for_each(|job| runner.job(job));
        if ui.quitting() {
            return Ok(());
        }
        let wanted = ui.app.window_title();
        if wanted != title {
            execute!(terminal.backend_mut(), SetTitle(&wanted))?;
            title = wanted;
        }
        let event = match ui.next_wake() {
            Some(wait) => match inbox.recv_timeout(wait) {
                Ok(event) => Some(event),
                Err(mpsc::RecvTimeoutError::Timeout) => None,
                Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
            },
            None => inbox.recv().ok(),
        };
        if let Some(event) = event {
            handle(ui, event);
        }
        // All that came in meanwhile, in one draw: a wheel sends rows of events.
        while let Ok(event) = inbox.try_recv() {
            handle(ui, event);
        }
    }
}

/// Runs until you quit.
pub fn run() -> io::Result<()> {
    if !(io::stdin().is_terminal() && stdout().is_terminal()) {
        return Err(io::Error::other("this needs a terminal to run in"));
    }
    let demo = std::env::var("KLETSE_RSS_DEMO").is_ok_and(|v| v == "1");
    // Before the screen changes: it says on stderr what was wrong with the config file.
    let backend = Arc::new(Backend::new(demo));
    let runtime = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build()?;

    let screen = Screen::enter()?;
    // It asks the terminal, so before anything else reads the answers.
    let picker = graphics();
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout()))?;
    let (events, inbox) = mpsc::channel();
    read_terminal(events.clone());
    std::thread::spawn(crate::content::highlight::warm_up);

    let mut ui = Ui::new(prefs::load(), demo, picker.clone());
    ui.start(backend.status());
    let runner = Runner { runtime, backend, picker, events };
    let result = event_loop(&mut terminal, &mut ui, &runner, &inbox);
    drop(screen);
    // Don't wait for what's still running: a picker reload that sleeps, an image.
    runner.runtime.shutdown_background();
    result
}
