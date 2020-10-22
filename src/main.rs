use argh::FromArgs;
use serde_json::Deserializer;
use std::{fs, io, ops::RangeInclusive};
use termion::{
    event::Key,
    input::{MouseTerminal, TermRead},
    raw::IntoRawMode,
    screen::AlternateScreen,
};
use tui::{
    backend::TermionBackend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    text::Spans,
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame, Terminal,
};
use unicode_width::UnicodeWidthStr;

#[cfg(feature = "dev-tools")]
use cpuprofiler::PROFILER;
use jed::{
    jq::{jv::JV, run_jq_query, JQ},
    shadow_tree::{
        construct_shadow_tree, next_displayable_line, prior_displayable_line, render_lines,
        renderable_lines, Shadow, ShadowTreeCursor,
    },
};
#[cfg(feature = "dev-tools")]
use prettytable::{cell, ptable, row, table, Table};

#[derive(FromArgs, PartialEq, Debug)]
/// Json viewer and editor
struct Args {
    #[cfg(feature = "dev-tools")]
    #[argh(subcommand)]
    mode: Mode,
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

// TODO
// * Large file perf (181 mb): 13.68 sec
//   * Initial parsing (serde): 3.77 sec
//   * Pre-rendering (lines): 2.29 sec (left and right)
//   * Query execution: 7.62 sec
//     * Serde -> JV: 3.38 sec
//     * Computing result: 0???? (it is the trivial filter)
//     * JV -> Serde: 3.37 sec
//   * Rendering is fast!
// * Improve memory estimates of actual json size.
// * Arrow key + emacs shortcuts for the query editor
// * Searching
// * Long strings
// * Edit tree, instead of 2 fixed panels
// * Saving
// * Speed up query serialization:
//   * Cut out serde entirely (except for parsing: hilariously, test -> serde -> jv appears to be
//   faster than text -> jv).
//   * Multithreaded serde -> jv
// Start with round trip between serde and jq, save jq -> lines for an optimization.
#[cfg(feature = "dev-tools")]
fn main() -> Result<(), io::Error> {
    let args: Args = argh::from_env();
    match args.mode {
        Mode::Normal(_) => run(args.json_path),
        Mode::Bench(_) => bench(args.json_path),
    }
}

#[cfg(not(feature = "dev-tools"))]
fn main() -> Result<(), io::Error> {
    let args: Args = argh::from_env();
    run(args.json_path)
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

fn run(json_path: String) -> Result<(), io::Error> {
    let stdin = io::stdin();
    let f = fs::File::open(json_path)?;
    let r = io::BufReader::new(f);
    let mut app = App::new(r)?;
    let stdout = io::stdout().into_raw_mode()?;
    let stdout = MouseTerminal::from(stdout);
    let stdout = AlternateScreen::from(stdout);
    let backend = TermionBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.draw(app.render(AppRenderMode::Normal))?;
    let mut rl: rustyline::Editor<()> = rustyline::Editor::new();
    // rl.bind_sequence(rustyline::KeyPress::Tab, rustyline::Cmd::Interrupt);
    for c in stdin.keys() {
        let c = c?;
        //
        match c {
            Key::Esc => break,
            Key::Char('q') => {
                terminal.draw(app.render(AppRenderMode::QueryEditor))?;
                match rl.readline_with_initial("", (&app.query, "")) {
                    Ok(new_query) => {
                        app.query = new_query;
                        // Just in case rustyline messed stuff up
                        force_draw(&mut terminal, app.render(AppRenderMode::Normal))?;
                        app.recompute_right();
                    }
                    Err(_) => {}
                }
            }
            Key::Char('\t') => app.focus = app.focus.swap(),
            _ => {}
        }
        let layout = JedLayout::new(&terminal.get_frame());
        let (view, view_rect) = match app.focus {
            Focus::Left => (Some(&mut app.left), layout.left),
            Focus::Right => (app.right.as_mut(), layout.right),
        };
        let line_limit = view_rect.height as usize - 2;
        match view {
            None => {}
            Some(View::Error(_)) => {}
            Some(View::Json(view)) => match c {
                Key::Down => {
                    if let Some(i) = view.cursor.as_mut() {
                        if let Some(new_i) =
                            next_displayable_line(*i, &view.shadow_tree, &view.values)
                        {
                            *i = new_i;
                        }
                        let i = *i; //Return mutable borrow
                        if !view.visible_range(line_limit).contains(&i) {
                            view.scroll =
                                next_displayable_line(view.scroll, &view.shadow_tree, &view.values)
                                    .expect("Shouldn't be able to scroll off the bottom");
                        }
                    }
                }
                Key::Up => {
                    if let Some(i) = view.cursor.as_mut() {
                        if let Some(new_i) =
                            prior_displayable_line(*i, &view.shadow_tree, &view.values)
                        {
                            *i = new_i;
                        }
                        let i = *i; //Return mutable borrow
                        if !view.visible_range(line_limit).contains(&i) {
                            view.scroll = prior_displayable_line(
                                view.scroll,
                                &view.shadow_tree,
                                &view.values,
                            )
                            .expect("Shouldn't be able to scroll off the bottom");
                        }
                    }
                }
                Key::Char('z') => {
                    if let Some(i) = view.cursor.as_mut() {
                        let mut cursor = ShadowTreeCursor::new(&view.shadow_tree, &view.values);
                        cursor
                            .seek(*i)
                            .expect("Cursor should not be able to reach an invalid index");
                        cursor.toggle_fold();
                        *i = cursor.index;
                    }
                }
                _ => {}
            },
        }
        terminal.draw(app.render(AppRenderMode::Normal))?;
    }
    Ok(())
}

#[cfg(feature = "dev-tools")]
fn bench(json_path: String) -> Result<(), io::Error> {
    let mut profiler = PROFILER.lock().unwrap();
    profiler.start("profile").unwrap();
    let f = fs::File::open(json_path)?;
    let r = io::BufReader::new(f);
    App::new(r)?;
    let mut profiler = PROFILER.lock().unwrap();
    profiler.stop().unwrap();
    Ok(())
}

#[derive(Copy, Clone, Eq, PartialEq)]
enum Focus {
    Left,
    Right,
}

impl Focus {
    fn swap(self) -> Self {
        match self {
            Focus::Left => Focus::Right,
            Focus::Right => Focus::Left,
        }
    }
}

struct App {
    left: View,
    right: Option<View>,
    focus: Focus,
    query: String,
}

#[derive(Debug, Clone)]
enum View {
    Json(JsonView),
    Error(Vec<String>),
}

impl View {
    fn new(values: Vec<JV>) -> Self {
        View::Json(JsonView::new(values))
    }
    fn render(&self, line_limit: u16, has_focus: bool) -> Paragraph {
        match self {
            View::Json(json_view) => json_view.render(line_limit, has_focus),
            View::Error(err) => {
                let err_text = err
                    .iter()
                    .flat_map(|e| e.split('\n'))
                    .map(Spans::from)
                    .collect::<Vec<_>>();
                Paragraph::new(err_text)
                    .style(Style::default().fg(Color::White).bg(Color::Red))
                    .alignment(Alignment::Left)
            }
        }
    }
}

#[derive(Debug, Clone)]
struct JsonView {
    scroll: usize,
    values: Vec<JV>,
    shadow_tree: Shadow,
    cursor: Option<usize>,
}

impl JsonView {
    fn new(values: Vec<JV>) -> Self {
        let shadow_tree = construct_shadow_tree(&values);
        let cursor = if values.is_empty() { None } else { Some(0) };
        JsonView {
            scroll: 0,
            values,
            shadow_tree,
            cursor,
        }
    }
    fn render(&self, line_limit: u16, has_focus: bool) -> Paragraph {
        let JsonView {
            shadow_tree,
            cursor,
            scroll,
            values,
            ..
        } = self;
        let cursor = if has_focus { *cursor } else { None };
        let text = render_lines(*scroll, line_limit, cursor, shadow_tree, values);
        Paragraph::new(text)
            .style(Style::default().fg(Color::White).bg(Color::Black))
            .alignment(Alignment::Left)
        //.wrap(Wrap { trim: false })
    }
    fn apply_query(&self, query: &str) -> View {
        match JQ::compile(query) {
            Ok(mut prog) => match run_jq_query(&self.values, &mut prog) {
                Ok(results) => View::Json(JsonView::new(results)),
                Err(err) => View::Error(vec![err]),
            },
            Err(err) => View::Error(err),
        }
    }
    fn visible_range(&self, line_limit: usize) -> RangeInclusive<usize> {
        let mut lines = renderable_lines(self.scroll, &self.shadow_tree, &self.values);
        let first = lines.next().expect("Should have at least one line").0;
        let last = lines
            .take(line_limit - 1)
            .last()
            .map_or(first, |(last, _)| last);
        first..=last
    }
}

struct JedLayout {
    left: Rect,
    right: Rect,
    query: Rect,
}

impl JedLayout {
    fn new<B: tui::backend::Backend>(f: &Frame<B>) -> JedLayout {
        let size = f.size();
        let vchunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(0), Constraint::Length(1)].as_ref())
            .split(size);
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Ratio(1, 2), Constraint::Ratio(1, 2)].as_ref())
            .split(vchunks[0]);
        JedLayout {
            left: chunks[0],
            right: chunks[1],
            query: vchunks[1],
        }
    }
}

enum AppRenderMode {
    Normal,
    QueryEditor,
}

impl App {
    fn new<R: io::Read>(r: R) -> io::Result<Self> {
        let content: Vec<JV> = Deserializer::from_reader(r)
            .into_iter::<JV>()
            .collect::<Result<Vec<JV>, _>>()?;
        let left = View::new(content);
        let mut app = App {
            left,
            right: None,
            focus: Focus::Left,
            query: String::new(),
        };
        app.recompute_right();
        Ok(app)
    }
    fn recompute_right(&mut self) {
        match &self.left {
            View::Json(left) => {
                self.right = Some(left.apply_query(&self.query));
            }
            View::Error(_) => {}
        }
    }
    fn render<B: tui::backend::Backend>(
        &mut self,
        mode: AppRenderMode,
    ) -> impl FnMut(&mut Frame<B>) + '_ {
        let App {
            left,
            right,
            query,
            focus,
            ..
        } = self;
        move |f| {
            let layout = JedLayout::new(f);
            let left_block = Block::default().title("Left").borders(Borders::ALL);
            let left_paragraph = left
                .render(layout.left.height, *focus == Focus::Left)
                .block(left_block);
            f.render_widget(left_paragraph, layout.left);
            let right_block = Block::default().title("Right").borders(Borders::ALL);
            match right {
                Some(right) => {
                    let right_paragraph = right
                        .render(layout.right.height, *focus == Focus::Right)
                        .block(right_block);
                    f.render_widget(right_paragraph, layout.right);
                }
                None => f.render_widget(right_block, layout.right),
            }
            match mode {
                AppRenderMode::Normal => {
                    let query = Paragraph::new(query.as_str())
                        .alignment(Alignment::Left)
                        .wrap(Wrap { trim: false });
                    f.render_widget(query, layout.query);
                }
                AppRenderMode::QueryEditor => {
                    f.set_cursor(0, layout.query.y);
                }
            }
        }
    }
}
