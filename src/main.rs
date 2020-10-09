use serde_json::{value::Value, Deserializer};
use std::{env, fs, io};
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
    widgets::{Block, Borders, Paragraph, Wrap},
    Terminal,
};
// TODO
// * Large file perf:
//   * Parsing is a bit slow
//   * Querying is slow
//   * Rendering is intolerably slow
// * To improve rendering:
//   * Pass the number of lines desired to render_lines, cut off once that's achieved
//   Folding is a bit tricky: probably store how many lines to skip if folded. Maybe do this before
//   implementing "retreat" so it'll be easier.
// * Arrow key + emacs shortcuts for the query editor
// * Make scrolling suck less
// * Edit tree, instead of 2 fixed panels
// * Saving
// * Modules
mod lines;
use lines::{json_to_lines, render_lines, Line, LineContent};
fn main() -> Result<(), io::Error> {
    let args: Vec<String> = env::args().collect();
    let stdin = io::stdin();
    let f = fs::File::open(&args[1])?;
    let r = io::BufReader::new(f);
    let mut app = App::new(r)?;
    app.render()?;
    let mut keys = stdin.keys();
    while let Some(c) = keys.next() {
        let view = &mut app.left;
        match c? {
            Key::Esc => break,
            Key::Down => {
                if let Some((value_ix, line_ix)) = view.cursor.as_mut() {
                    let current_value = &view.lines[*value_ix];
                    if *line_ix == current_value.len() - 1 {
                        if *value_ix < view.lines.len() - 1 {
                            *value_ix += 1;
                            *line_ix = 0;
                        }
                    } else {
                        let line = &current_value[*line_ix];
                        match line.content {
                            LineContent::ArrayStart(skipped_lines) if line.folded => {
                                *line_ix += 2 + skipped_lines;
                            }
                            LineContent::ObjectStart(skipped_lines) if line.folded => {
                                *line_ix += 2 + skipped_lines;
                            }
                            _ => {
                                *line_ix += 1;
                            }
                        }
                    }
                }
            }
            Key::Up => {
                if let Some((value_ix, line_ix)) = view.cursor.as_mut() {
                    if *line_ix == 0 {
                        if *value_ix > 0 {
                            *value_ix -= 1;
                            *line_ix = view.lines[*value_ix].len() - 1;
                        }
                    } else {
                        *line_ix -= 1;
                    }
                    let line = &view.lines[*value_ix][*line_ix];
                    if let LineContent::ArrayEnd(skipped_lines)
                    | LineContent::ObjectEnd(skipped_lines) = line.content
                    {
                        let matching_line_ix = *line_ix - 1 - skipped_lines;
                        let matching_line = &view.lines[*value_ix][matching_line_ix];
                        if matching_line.folded {
                            *line_ix = matching_line_ix;
                        }
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
                if let Some((value_ix, line_ix)) = view.cursor.as_mut() {
                    let line = &mut view.lines[*value_ix][*line_ix];
                    match line.content {
                        LineContent::ArrayStart(_) | LineContent::ObjectStart(_) => {
                            line.folded = !line.folded;
                        }
                        LineContent::ArrayEnd(skipped_lines)
                        | LineContent::ObjectEnd(skipped_lines) => {
                            *line_ix -= skipped_lines + 1;
                            let line = &mut view.lines[*value_ix][*line_ix];
                            assert_eq!(line.folded, false);
                            line.folded = true;
                        }
                        _ => {}
                    }
                }
            }
            Key::Char('q') => {
                app.new_query = Some(app.query.clone());
                app.render()?;
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
                            app.render()?;
                        }
                        Key::Char(c) => {
                            new_query.push(c);
                            app.render()?;
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
        app.render()?;
    }
    Ok(())
}

type Screen = AlternateScreen<MouseTerminal<RawTerminal<io::Stdout>>>;

struct App {
    terminal: Terminal<TermionBackend<Screen>>,
    left: View,
    right: Option<View>,
    new_query: Option<String>,
    query: String,
}

#[derive(Debug, Clone)]
struct View {
    scroll: usize,
    values: Vec<Value>,
    lines: Vec<Vec<Line>>,
    cursor: Option<(usize, usize)>,
}

impl View {
    fn new(values: Vec<Value>) -> Self {
        let lines = json_to_lines(values.iter());
        let cursor = if lines.is_empty() { None } else { Some((0, 0)) };
        View {
            scroll: 0,
            values,
            lines,
            cursor,
        }
    }
    fn render(&self) -> Paragraph {
        let View {
            lines,
            cursor,
            scroll,
            ..
        } = self;
        let text = render_lines(*scroll, cursor, lines);
        Paragraph::new(text)
            .style(Style::default().fg(Color::White).bg(Color::Black))
            .alignment(Alignment::Left)
        //.wrap(Wrap { trim: false })
    }
    fn apply_query(&self, query: &str) -> Self {
        let mut prog = jq_rs::compile(query).expect("jq compilation error");
        let right_strings: Vec<String> = self
            .values
            .iter()
            .map(|j| prog.run(&j.to_string()).expect("jq execution error"))
            .collect();
        let right_content: Result<Vec<Value>, _> = right_strings
            .iter()
            .flat_map(|j| Deserializer::from_str(j).into_iter::<Value>())
            .collect();
        let values = right_content.expect("json decoding error");
        View::new(values)
    }
}

impl App {
    fn new<R: io::Read>(r: R) -> io::Result<Self> {
        let content: Vec<Value> = Deserializer::from_reader(r)
            .into_iter::<Value>()
            .collect::<Result<Vec<Value>, _>>()?;
        let left = View::new(content);
        let stdout = io::stdout().into_raw_mode()?;
        let stdout = MouseTerminal::from(stdout);
        let stdout = AlternateScreen::from(stdout);
        let backend = TermionBackend::new(stdout);
        let terminal = Terminal::new(backend)?;
        let mut app = App {
            terminal,
            left,
            right: None,
            new_query: None,
            query: String::new(),
        };
        app.recompute_right();
        Ok(app)
    }
    fn recompute_right(&mut self) {
        self.right = Some(self.left.apply_query(&self.query));
    }
    fn render(&mut self) -> io::Result<()> {
        let App {
            terminal,
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
            let left_paragraph = left.render().block(left_block);
            f.render_widget(left_paragraph, chunks[0]);
            let right_block = Block::default().title("Right").borders(Borders::ALL);
            match right {
                Some(right) => {
                    let right_paragraph = right.render().block(right_block);
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
