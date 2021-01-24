use argh::FromArgs;
use crossterm::{
    event,
    event::KeyCode,
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use jex::{
    app::{App, AppRenderMode, Focus},
    cursor::GlobalCursor,
    layout::JexLayout,
    view_tree::View,
};
use log::debug;
use regex::Regex;
use simplelog::WriteLogger;
use std::{default::Default, fs, fs::File, io, io::Write, panic};
use tui::{
    backend::CrosstermBackend,
    layout::Rect,
    widgets::{Block, Borders},
    Frame, Terminal,
};
use unicode_width::UnicodeWidthStr;

#[cfg(feature = "dev-tools")]
use cpuprofiler::PROFILER;
#[cfg(feature = "dev-tools")]
use prettytable::{cell, ptable, row, table, Table};

#[derive(FromArgs, PartialEq, Debug)]
/// Json viewer and editor
struct Args {
    #[cfg(feature = "dev-tools")]
    #[argh(subcommand)]
    mode: Mode,
    #[argh(option)]
    #[argh(description = "logging level")]
    #[argh(default = "log::LevelFilter::Warn")]
    log_level: log::LevelFilter,
    #[argh(option)]
    #[argh(description = "logging output file")]
    log_path: Option<String>,
    #[argh(positional)]
    json_path: String,
}

#[derive(FromArgs, PartialEq, Debug)]
#[argh(subcommand)]
enum Mode {
    Normal(NormalMode),
    Bench(BenchMode),
}

#[derive(FromArgs, PartialEq, Debug)]
#[argh(subcommand, name = "load")]
/// Run the editor
struct NormalMode {}

#[derive(FromArgs, PartialEq, Debug)]
#[argh(subcommand, name = "bench")]
/// Benchmark loading a json file
struct BenchMode {}

// Large file perf (181 mb):
// * Old: 13.68 sec
//   * Initial parsing (serde): 3.77 sec
//   * Pre-rendering (lines): 2.29 sec (left and right)
//   * Query execution: 7.62 sec
//     * Serde -> JV: 3.38 sec
//     * Computing result: 0???? (it is the trivial filter)
//     * JV -> Serde: 3.37 sec
// * New: 6.32 sec
//   * Initial parsing (JV deserialize): 6.26
//   * Query execution: ~0
//
// What can we do to improve load times? The current situation looks bleak.
// * If (big if) JV iterated through maps in insertion order, you could imagine rendinering the
// scene before the file is fully loaded. We can't load instantly, but we can definitely load one
// page of json instantly. Probably worth reading the JV object implementation: hopefully it's not
// too complicated.
// * We might be able to deserialize in parallel.
// * Use private JV functions to bypass typechecking when we already know the type.
// * Only use JVRaws duing deserialization.
// * Stop using JQ entirely (this would be hellish)
// * If you can guarantee identiacal rendering from JV and serde Values, deserialize into a serde
// Value (faster), become interactive then, and secretly swap in the JV once that's ready. Not
// great from a memory perspective. Any way to do that incrementally? Since we'd have full control
// over the value-like structure, it might be doable. Shared mutable access across different
// threads is.... a concenrn.
// * Completely violate the JV privacy boundary and construct JVs directly. Would we be able to
// make it faster? I'd be surprised: my guess is that the JV implementation is fairly optimal
// _given_ the datastructure, which we wouldn't be able to avoid.
// * Write an interpreter for JQ bytecode. That's definitely considered an implementation detail,
// so that would be pretty evil, but we might be able to operate directly on serde Values.
//
// TODO
// * Long keys: once you can wrap keys across multiple lines, you have the tools to guarantee that
//   that the content width never falls below 7.
// * Edit tree:
//   * Children can be modified if they have no children
//   * Allow copying descendents onto another root, so you if you want to modify a tree's root you
// can do so by making a new root and then copying over the descendents
// * Error messages (no search results, can't fold a leaf, can't edit a non-leaf)
// * Saving
// * Rename current view
// * Diffs

#[cfg(feature = "dev-tools")]
fn main() -> Result<(), io::Error> {
    use coredump;
    coredump::register_panic_handler();
    let args: Args = argh::from_env();
    init_logging(&args);
    match args.mode {
        Mode::Normal(_) => run(args.json_path),
        Mode::Bench(_) => bench(args.json_path),
    }
}

#[cfg(not(feature = "dev-tools"))]
fn main() -> Result<(), io::Error> {
    let args: Args = argh::from_env();
    init_logging(&args);
    run(args.json_path)
}

fn init_logging(args: &Args) {
    if let Some(path) = args.log_path.as_ref() {
        let fout = File::create(path).expect("Couldn't create log file");
        WriteLogger::init(args.log_level, Default::default(), fout)
            .expect("Couldn't initalize logger");
    }
}

fn force_draw<B: tui::backend::Backend, F: FnMut(&mut Frame<B>)>(
    terminal: &mut Terminal<B>,
    mut f: F,
) -> Result<(), io::Error> {
    terminal.autoresize()?;
    let mut frame = terminal.get_frame();
    f(&mut frame);
    let current_buffer = terminal.current_buffer_mut().clone();
    terminal.current_buffer_mut().reset();
    terminal.draw(f)?;
    let area = current_buffer.area;
    let width = area.width;

    let mut updates: Vec<(u16, u16, &tui::buffer::Cell)> = vec![];
    // Cells from the current buffer to skip due to preceeding multi-width characters taking their
    // place (the skipped cells should be blank anyway):
    let mut to_skip: usize = 0;
    for (i, current) in current_buffer.content.iter().enumerate() {
        if to_skip == 0 {
            let x = i as u16 % width;
            let y = i as u16 / width;
            updates.push((x, y, &current_buffer.content[i]));
        }

        to_skip = current.symbol.width().saturating_sub(1);
    }
    terminal.backend_mut().draw(updates.into_iter())
}

struct DeferRestoreTerminal {}

impl Drop for DeferRestoreTerminal {
    fn drop(&mut self) {
        disable_raw_mode().expect("Failed to disable raw mode");
        execute!(io::stdout(), LeaveAlternateScreen).expect("Failed to leave alternate screen");
    }
}

fn run(json_path: String) -> Result<(), io::Error> {
    enable_raw_mode().expect("Failed to enter raw mode");

    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen).expect("Failed to enter alternate screen");
    let default_panic_handler = panic::take_hook();
    panic::set_hook(Box::new(move |p| {
        disable_raw_mode().expect("Failed to disable raw mode");
        execute!(io::stdout(), LeaveAlternateScreen).expect("Failed to leave alternate screen");
        default_panic_handler(p);
    }));
    let _defer = DeferRestoreTerminal {};
    let f = fs::File::open(&json_path)?;
    let r = io::BufReader::new(f);
    let stdout = io::stdout();
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    let initial_layout = JexLayout::new(terminal.get_frame().size(), false);
    let mut app = App::new(r, json_path, initial_layout)?;
    terminal.draw(app.render(AppRenderMode::Normal))?;
    let mut query_rl: rustyline::Editor<()> = rustyline::Editor::new();
    let mut search_rl: rustyline::Editor<()> = rustyline::Editor::new();
    let mut title_rl: rustyline::Editor<()> = rustyline::Editor::new();
    query_rl.bind_sequence(rustyline::KeyPress::Esc, rustyline::Cmd::Interrupt);
    search_rl.bind_sequence(rustyline::KeyPress::Esc, rustyline::Cmd::Interrupt);
    title_rl.bind_sequence(rustyline::KeyPress::Esc, rustyline::Cmd::Interrupt);
    loop {
        let event = event::read().expect("Error getting next event");
        debug!("Event: {:?}", event);
        let c = match event {
            event::Event::Key(c) => c,
            event::Event::Mouse(_) => panic!("Mouse events aren't enabled!"),
            event::Event::Resize(width, height) => {
                let rect = Rect {
                    x: 0,
                    y: 0,
                    width,
                    height,
                };
                let layout = JexLayout::new(rect, app.show_tree);
                app.resize(layout);
                terminal.draw(app.render(AppRenderMode::Normal))?;
                continue;
            }
        };
        let layout = JexLayout::new(terminal.get_frame().size(), app.show_tree);
        match c.code {
            KeyCode::Esc => break,
            KeyCode::Char('t') => {
                app.show_tree = !app.show_tree;
            }
            KeyCode::Char('q') => {
                terminal.draw(app.render(AppRenderMode::InputEditor))?;
                let (_, _, query) = app.current_views_mut();
                match query_rl.readline_with_initial("", (&*query, "")) {
                    Ok(new_query) => {
                        *query = new_query;
                        // Just in case rustyline messed stuff up
                        force_draw(&mut terminal, app.render(AppRenderMode::Normal))?;
                        app.recompute_right(layout.right);
                    }
                    Err(_) => {}
                }
            }
            KeyCode::Tab => {
                app.focus = app.focus.swap();
                debug!("Swapped focus to {:?}", app.focus);
            }
            KeyCode::Char('+') => {
                if let Focus::Right = app.focus {
                    app.index.parent.push(app.index.child);
                };
                let tree = app
                    .views
                    .index_tree_mut(&app.index.parent)
                    .expect("App index invalidated");
                app.index.child = tree.children.len();
                tree.push_trivial_child(layout.right);
            }
            KeyCode::Char('j') => {
                app.index.advance(&app.views);
            }
            KeyCode::Char('k') => {
                app.index.regress();
            }
            KeyCode::Char('r') => {
                terminal.draw(app.render(AppRenderMode::InputEditor))?;
                let view_frame = app.focused_view_mut();
                match title_rl.readline_with_initial("New Title:", (&view_frame.name, "")) {
                    Ok(new_name) => {
                        view_frame.name = new_name;
                    }
                    Err(_) => {}
                }
                force_draw(&mut terminal, app.render(AppRenderMode::Normal))?;
            }
            KeyCode::Char('s') => {
                terminal.draw(app.render(AppRenderMode::InputEditor))?;
                let view_frame = app.focused_view();
                if let View::Json(Some(view)) = &view_frame.view {
                    match title_rl.readline_with_initial("Save to:", (&view_frame.name, "")) {
                        Ok(path) => {
                            view.save_to(&path);
                        }
                        Err(_) => {}
                    }
                }
                force_draw(&mut terminal, app.render(AppRenderMode::Normal))?;
            }
            _ => {}
        }
        let view_rect = match app.focus {
            Focus::Left => layout.left,
            Focus::Right => layout.right,
        };
        let view_frame = app.focused_view_mut();
        let json_rect = Block::default().borders(Borders::ALL).inner(view_rect);
        match &mut view_frame.view {
            View::Error(_) => {}
            View::Json(None) => {}
            View::Json(Some(view)) => {
                view.resize_to(json_rect);
                match c.code {
                    KeyCode::Down => {
                        view.advance_cursor();
                    }
                    KeyCode::Up => {
                        view.regress_cursor();
                    }
                    KeyCode::Char('z') => {
                        view.toggle_fold();
                    }
                    KeyCode::Char('/') => {
                        terminal.draw(app.render(AppRenderMode::InputEditor))?;
                        match search_rl.readline_with_initial("Search:", ("", "")) {
                            Ok(new_search) => {
                                // Just in case rustyline messed stuff up
                                force_draw(&mut terminal, app.render(AppRenderMode::Normal))?;
                                app.search_re = Regex::new(new_search.as_ref()).ok();
                                app.search(false);
                            }
                            Err(_) => {}
                        }
                    }
                    KeyCode::Char('n') => {
                        app.search(false);
                    }
                    KeyCode::Char('N') => {
                        app.search(true);
                    }
                    KeyCode::Home => {
                        view.scroll = GlobalCursor::new(view.values.clone(), view.rect.width)
                            .expect("values should still exist");
                        view.cursor = view.scroll.value_cursor.clone();
                    }
                    KeyCode::End => {
                        view.scroll = GlobalCursor::new_end(view.values.clone(), view.rect.width)
                            .expect("values should still exist");
                        view.cursor = view.scroll.value_cursor.clone();
                    }
                    _ => {}
                };
            }
        }
        terminal.draw(app.render(AppRenderMode::Normal))?;
    }
    // Gracefully freeing the JV values can take a significant amount of time and doesn't actually
    // benefit anything: the OS will clean up after us when we exit.
    std::mem::forget(app);
    Ok(())
}

#[cfg(feature = "dev-tools")]
fn bench(json_path: String) -> Result<(), io::Error> {
    let mut profiler = PROFILER.lock().unwrap();
    profiler.start("profile").unwrap();
    let f = fs::File::open(&json_path)?;
    let r = io::BufReader::new(f);
    let initial_layout = JexLayout {
        left: Rect {
            x: 0,
            y: 0,
            width: 100,
            height: 100,
        },
        right: Rect {
            x: 100,
            y: 0,
            width: 100,
            height: 100,
        },
        query: Rect {
            x: 0,
            y: 100,
            width: 100,
            height: 1,
        },
        tree: None,
    };
    let mut app = App::new(r, json_path, initial_layout)?;
    std::mem::forget(app);
    profiler.stop().unwrap();
    Ok(())
}
