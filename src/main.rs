use argh::FromArgs;
use cpuprofiler::PROFILER;
use serde_json::{value::Value, Deserializer};
use std::{fs, io};
use termion::{
    event::Key,
    input::{MouseTerminal, TermRead},
    raw::{IntoRawMode, RawTerminal},
    screen::AlternateScreen,
};
use tui::{
    backend::TermionBackend,
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Style},
    text::{Span, Spans},
    widgets::{Block, Borders, Paragraph, Wrap},
    Terminal,
};
#[derive(FromArgs, PartialEq, Debug)]
/// A command with positional arguments.
struct Args {
    #[argh(positional)]
    json_path: String,

    #[argh(switch, description = "load the file and then quit")]
    bench: bool,
}
// TODO
// * Large file perf (181 mb): 13.68 sec
//   * Initial parsing (serde): 3.77 sec
//   * Pre-rendering (lines): 2.29 sec (left and right)
//   * Query execution: 7.62 sec
//     * Serde -> JV: 3.38 sec
//     * Computing result: 0???? (it is the trivial filter)
//     * JV -> Serde: 3.37 sec
//   * Rendering is fast!
// * Arrow key + emacs shortcuts for the query editor
// * Make scrolling suck less
// * Switch panels
// * Edit tree, instead of 2 fixed panels
// * Saving
// * Speed up query serialization:
//   * Cut out serde entirely (except for parsing: hilariously, test -> serde -> jv appears to be
//   faster than text -> jv).
//   * Multithreaded serde -> jv
// Start with round trip between serde and jq, save jq -> lines for an optimization.
use jed::{
    jq::{run_jq_query, JQ},
    lines::{
        json_to_lines, next_displayable_line, prior_displayable_line, render_lines, Line,
        LineContent,
    },
};
fn main() -> Result<(), io::Error> {
    let args: Args = argh::from_env();
    if args.bench {
        let mut profiler = PROFILER.lock().unwrap();
        profiler.start("profile").unwrap();
    };
    let stdin = io::stdin();
    let f = fs::File::open(args.json_path)?;
    let r = io::BufReader::new(f);
    let mut app = App::new(r)?;
    if args.bench {
        let mut profiler = PROFILER.lock().unwrap();
        profiler.stop().unwrap();
        return Ok(());
    }
    let stdout = io::stdout().into_raw_mode()?;
    let stdout = MouseTerminal::from(stdout);
    let stdout = AlternateScreen::from(stdout);
    let backend = TermionBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    app.render(&mut terminal)?;
    let mut keys = stdin.keys();
    while let Some(c) = keys.next() {
        let c = c?;
        //
        match c {
            Key::Esc => break,
            Key::Char('q') => {
                app.new_query = Some(app.query.clone());
                app.render(&mut terminal)?;
                #[allow(clippy::while_let_on_iterator)]
                while let Some(key) = keys.next() {
                    let new_query = app.new_query.as_mut().unwrap();
                    match key? {
                        Key::Esc => break,
                        Key::Char('\n') => {
                            app.query = app.new_query.take().unwrap();
                            app.recompute_right();
                            break;
                        }
                        Key::Backspace => {
                            new_query.pop();
                            app.render(&mut terminal)?;
                        }
                        Key::Char(c) => {
                            new_query.push(c);
                            app.render(&mut terminal)?;
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
        let view = &mut app.left;
        match view {
            View::Error(_) => {}
            View::Json(view) => match c {
                Key::Down => {
                    if let Some(i) = view.cursor.as_mut() {
                        if let Some(new_i) = next_displayable_line(*i, &view.lines) {
                            *i = new_i;
                        }
                    }
                }
                Key::Up => {
                    if let Some(i) = view.cursor.as_mut() {
                        if let Some(new_i) = prior_displayable_line(*i, &view.lines) {
                            *i = new_i;
                        }
                    }
                }
                Key::Char('j') => {
                    view.scroll += 1;
                }
                Key::Char('k') => {
                    view.scroll = view.scroll.saturating_sub(1);
                }
                Key::Char('z') => {
                    if let Some(i) = view.cursor.as_mut() {
                        let line = &mut view.lines[*i];
                        match line.content {
                            LineContent::ArrayStart(_) | LineContent::ObjectStart(_) => {
                                line.folded = !line.folded;
                            }
                            LineContent::ArrayEnd(skipped_lines)
                            | LineContent::ObjectEnd(skipped_lines) => {
                                *i -= skipped_lines + 1;
                                let line = &mut view.lines[*i];
                                assert_eq!(line.folded, false);
                                line.folded = true;
                            }
                            _ => {}
                        }
                    }
                }
                _ => {}
            },
        }
        app.render(&mut terminal)?;
    }
    Ok(())
}

type Screen = AlternateScreen<MouseTerminal<RawTerminal<io::Stdout>>>;

struct App {
    left: View,
    right: Option<View>,
    new_query: Option<String>,
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
    fn render(&self, line_limit: u16) -> Paragraph {
        match self {
            View::Json(json_view) => json_view.render(line_limit),
            View::Error(err) => {
                let err_text = err
                    .into_iter()
                    .flat_map(|e| e.split('\n'))
                    .map(|e| Spans::from(e))
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
    lines: Vec<Line>,
    cursor: Option<usize>,
}

impl JsonView {
    fn new(values: Vec<Value>) -> Self {
        let lines = json_to_lines(values.iter());
        let cursor = if lines.is_empty() { None } else { Some(0) };
        JsonView {
            scroll: 0,
            values,
            lines,
            cursor,
        }
    }
    fn render(&self, line_limit: u16) -> Paragraph {
        let JsonView {
            lines,
            cursor,
            scroll,
            ..
        } = self;
        let text = render_lines(*scroll, line_limit, *cursor, lines);
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
            new_query: None,
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
    fn render(&mut self, terminal: &mut Terminal<TermionBackend<Screen>>) -> io::Result<()> {
        let App {
            left,
            right,
            query,
            new_query,
            ..
        } = self;
        terminal.draw(|f| {
            let size = f.size();
            let vchunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(0), Constraint::Length(1)].as_ref())
                .split(size);
            let chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Ratio(1, 2), Constraint::Ratio(1, 2)].as_ref())
                .split(vchunks[0]);
            let left_block = Block::default().title("Left").borders(Borders::ALL);
            let left_paragraph = left.render(chunks[0].height).block(left_block);
            f.render_widget(left_paragraph, chunks[0]);
            let right_block = Block::default().title("Right").borders(Borders::ALL);
            match right {
                Some(right) => {
                    let right_paragraph = right.render(chunks[1].height).block(right_block);
                    f.render_widget(right_paragraph, chunks[1]);
                }
                None => f.render_widget(right_block, chunks[1]),
            }
            let query = Paragraph::new(new_query.as_ref().unwrap_or(query).as_str())
                .alignment(Alignment::Left)
                .wrap(Wrap { trim: false });
            if let Some(query) = new_query.as_ref() {
                f.set_cursor(query.len() as u16, vchunks[1].y);
            }
            f.render_widget(query, vchunks[1]);
        })
    }
}
