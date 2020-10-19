use argh::FromArgs;
use serde_json::{value::Value, Deserializer};
use std::{fs, io, io::Write, ops::RangeInclusive};
use termion::{
    event::Key,
    input::{MouseTerminal, TermRead},
    raw::{IntoRawMode, RawTerminal},
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
#[cfg(feature = "dev-tools")]
use jed::lines::memory::{MemoryStat, MemoryStats};
use jed::{
    jq::{run_jq_query, JQ},
    shadow_tree,
    shadow_tree::{
        construct_shadow_tree, next_displayable_line, prior_displayable_line, render_lines,
        renderable_lines, Shadow,
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
    Memory(MemoryMode),
    Sizes(SizesMode),
}

#[derive(FromArgs, PartialEq, Debug)]
#[argh(subcommand, name = "load")]
/// Run the editor
struct NormalMode {}

#[derive(FromArgs, PartialEq, Debug)]
#[argh(subcommand, name = "bench")]
/// Benchmark loading a json file
struct BenchMode {}

#[derive(FromArgs, PartialEq, Debug)]
#[argh(subcommand, name = "memory")]
/// Break down memory usage from loading a json file
struct MemoryMode {}

#[derive(FromArgs, PartialEq, Debug)]
#[argh(subcommand, name = "sizes")]
/// Print the sizes of various data structures
struct SizesMode {}

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
        Mode::Memory(_) => memory(args.json_path),
        Mode::Sizes(_) => sizes(),
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
    let mut keys = stdin.keys();
    while let Some(c) = keys.next() {
        let c = c?;
        //
        match c {
            Key::Esc => break,
            Key::Char('q') => {
                terminal.draw(app.render(AppRenderMode::QueryEditor))?;
                match rl.readline_with_initial("", (&app.query, "")) {
                    Ok(new_query) => {
                        app.query = new_query;
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
                        let (new_i, shadow) = shadow_tree::mutable::index_shadow(
                            *i,
                            &mut view.shadow_tree,
                            &view.values,
                        )
                        .expect("Cursor should not be able to reach an invalid index");
                        *i = new_i;
                        shadow.folded = !shadow.folded;
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

#[cfg(feature = "dev-tools")]
fn percent(num: usize, denom: usize) -> String {
    format!("{:2.0}%", (num * 100) as f64 / denom as f64)
}

#[cfg(feature = "dev-tools")]
fn memory(json_path: String) -> Result<(), io::Error> {
    let f = fs::File::open(json_path)?;
    let r = io::BufReader::new(f);
    let app = App::new(r)?;
    let view = match app.left {
        View::Json(v) => v,
        View::Error(_) => panic!("Expected non-error view"),
    };
    let memory_stats = MemoryStats::from_lines(&view.lines);
    let line_size = std::mem::size_of::<Line>();
    let mut line_stats = vec![
        ("Null", memory_stats.null),
        ("Bool", memory_stats.bool),
        ("Number", memory_stats.number),
        ("String", memory_stats.string),
        ("ArrayStart", memory_stats.array_start),
        ("ArrayEnd", memory_stats.array_end),
        ("ObjectStart", memory_stats.object_start),
        ("ObjectEnd", memory_stats.object_end),
        ("ValueTerminator", memory_stats.value_terminator),
        ("Key", memory_stats.key),
    ];
    let comma_count = memory_stats.null.count
        + memory_stats.bool.count
        + memory_stats.number.count
        + memory_stats.string.count
        - memory_stats.value_terminator.count;
    line_stats.push((
        "Comma",
        MemoryStat {
            count: comma_count,
            indirect_bytes: 0,
            json_size: comma_count,
        },
    ));
    let mut lines_total = MemoryStat::default();
    for (_, stat) in line_stats.iter() {
        lines_total += *stat;
    }
    line_stats.push(("Total", lines_total));
    let mut direct_table = Table::new();
    direct_table.add_row(row!["Type", "Count", "Bytes", "Fraction"]);
    for (ty, stat) in &line_stats {
        direct_table.add_row(row![
            ty,
            stat.count,
            stat.count * line_size,
            percent(stat.count, lines_total.count)
        ]);
    }
    println!(
        "Direct memory usage {}",
        percent(
            lines_total.count * line_size,
            lines_total.count * line_size + lines_total.indirect_bytes
        )
    );
    direct_table.printstd();
    println!(
        "Indirect memory usage {}",
        percent(
            lines_total.indirect_bytes,
            lines_total.count * line_size + lines_total.indirect_bytes
        )
    );
    ptable!(
        ["Type", "Bytes", "Fraction"],
        [
            "String",
            memory_stats.string.indirect_bytes,
            percent(
                memory_stats.string.indirect_bytes,
                lines_total.indirect_bytes
            )
        ],
        [
            "Key",
            memory_stats.string.indirect_bytes,
            percent(memory_stats.key.indirect_bytes, lines_total.indirect_bytes)
        ]
    );
    let mut json_table = Table::new();
    json_table.add_row(row!["Type", "Bytes", "Fraction"]);
    for (ty, stat) in &line_stats {
        json_table.add_row(row![
            ty,
            stat.json_size,
            percent(stat.json_size, lines_total.json_size)
        ]);
    }
    println!(
        "Original JSON (estimated) {}",
        percent(
            lines_total.json_size,
            lines_total.count * line_size + lines_total.indirect_bytes
        )
    );
    json_table.printstd();
    Ok(())
}

#[cfg(feature = "dev-tools")]
fn sizes() -> Result<(), io::Error> {
    use serde_json::{map::Map, Number};
    use std::mem::size_of;
    dbg!(size_of::<Line>());
    dbg!(size_of::<LineContent>());
    dbg!(size_of::<Option<String>>());
    dbg!(size_of::<Option<Box<str>>>());
    dbg!(size_of::<usize>());
    dbg!(size_of::<Number>());
    dbg!(size_of::<Value>());
    dbg!(size_of::<Map<String, Value>>());
    dbg!(size_of::<jed::shadow_tree::Shadow>());
    Ok(())
}

type Screen = AlternateScreen<MouseTerminal<RawTerminal<io::Stdout>>>;

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
    fn new(values: Vec<Value>) -> Self {
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
    values: Vec<Value>,
    shadow_tree: Shadow,
    cursor: Option<usize>,
}

impl JsonView {
    fn new(values: Vec<Value>) -> Self {
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
        let content: Vec<Value> = Deserializer::from_reader(r)
            .into_iter::<Value>()
            .collect::<Result<Vec<Value>, _>>()?;
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
    fn render<'a, B: tui::backend::Backend>(
        &'a mut self,
        mode: AppRenderMode,
    ) -> impl FnMut(&mut Frame<B>) + 'a {
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
