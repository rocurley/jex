use ego_tree::{NodeId, NodeRef, Tree};
use serde_json::{value::Value, Deserializer};
use std::{env, fs, io, iter::once};
use termion::{
    event::Key,
    input::{MouseTerminal, TermRead},
    raw::{IntoRawMode, RawTerminal},
    screen::AlternateScreen,
};
use tui::{
    backend::TermionBackend,
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Span, Spans},
    widgets::{Block, Borders, Paragraph, Wrap},
    Terminal,
};
// TODO
// * Large file perf:
//   * Parsing is a bit slow
//   * Querying is slow
//   * Rendering is intolerably slow
// * To improve rendering:
//   * To allow partial rendering, make it possible to start rendering midway through the json
//   (done)
//   * Need to implement "retreat" to undo "advance" so you can use a JsonText to store the current
//   scroll state
//   * Maybe we can swap out the tree for a flat array? Moving back and forth would be easier.
//   Folding is a bit tricky: probably store how many lines to skip if folded. Maybe do this before
//   implementing "retreat" so it'll be easier.
// * Arrow key + emacs shortcuts for the query editor
// * Make scrolling suck less
// * Edit tree, instead of 2 fixed panels
// * Saving
// * Modules
mod tree;
use tree::{jsons_to_trees, last_node, next_node, prior_node, Node, PseudoNode};
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
                if let Some(focus) = view.focus.as_mut() {
                    let focus_node = view.content[focus.0].get(focus.1).expect("Invalid focus");
                    match next_node(focus_node) {
                        Some(next) => focus.1 = next.id(),
                        None if focus.0 + 1 == view.content.len() => {}
                        None => {
                            *focus = (focus.0 + 1, view.content[focus.0 + 1].root().id());
                        }
                    }
                }
            }
            Key::Up => {
                if let Some(focus) = view.focus.as_mut() {
                    let focus_node = view.content[focus.0].get(focus.1).expect("Invalid focus");
                    match prior_node(focus_node) {
                        Some(prior) => focus.1 = prior.id(),
                        None if focus.0 == 0 => {}
                        None => {
                            let focus_node = last_node(&view.content[focus.0 - 1]);
                            *focus = (focus.0 - 1, focus_node.id());
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
                if let Some(focus) = view.focus {
                    let mut focus_node = view.content[focus.0]
                        .get_mut(focus.1)
                        .expect("Invalid focus");
                    let node = focus_node.value();
                    node.folded = !node.folded;
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

struct JsonText<'a> {
    indent: usize,
    v: Option<NodeRef<'a, PseudoNode>>,
    close: bool,
    focus: Option<NodeId>,
}
impl<'a> JsonText<'a> {
    fn advance(&mut self) {
        let v = match self.v {
            None => return,
            Some(v) => v,
        };
        if !self.close && !v.value().folded {
            if let Some(child) = v.first_child() {
                self.v = Some(child);
                self.indent += 1;
                return;
            }
            if let Node::Array | Node::Object = v.value().node {
                self.close = true;
                return;
            }
        }
        if let Some(sib) = v.next_sibling() {
            self.close = false;
            self.v = Some(sib);
            return;
        }
        self.v = v.parent();
        if self.v.is_some() {
            self.close = true;
            self.indent -= 1;
        }
    }
}

impl<'a> Iterator for JsonText<'a> {
    type Item = Vec<Span<'a>>;
    fn next(&mut self) -> Option<Vec<Span<'a>>> {
        let v = self.v?;
        let has_comma = v.next_sibling().is_some();
        let indent = Span::raw("  ".repeat(self.indent));
        let mut out = match &v.value().key {
            Some(key) if !self.close => {
                vec![indent, Span::raw(format!("{:?}", key)), Span::raw(" : ")]
            }
            _ => vec![indent],
        };
        let style = if Some(v.id()) == self.focus {
            Style::default().bg(Color::Blue)
        } else {
            Style::default()
        };
        match v.value() {
            PseudoNode {
                node: Node::Null, ..
            } => {
                out.push(Span::styled("null", style));
                if has_comma {
                    out.push(Span::raw(","));
                }
            }
            PseudoNode {
                node: Node::String(s),
                ..
            } => {
                out.push(Span::styled(format!("{:?}", s), style));
                if has_comma {
                    out.push(Span::raw(","));
                }
            }
            PseudoNode {
                node: Node::Bool(b),
                ..
            } => {
                out.push(Span::styled(b.to_string(), style));
                if has_comma {
                    out.push(Span::raw(","));
                }
            }
            PseudoNode {
                node: Node::Number(x),
                ..
            } => {
                out.push(Span::styled(x.to_string(), style));
                if has_comma {
                    out.push(Span::raw(","));
                }
            }
            PseudoNode {
                node: Node::Array,
                folded: true,
                ..
            } => {
                out.push(Span::styled("[...]", style));
                if has_comma {
                    out.push(Span::raw(","));
                }
                out.push(Span::styled(
                    format!(" ({} items)", v.children().count()),
                    Style::default().add_modifier(Modifier::DIM),
                ));
            }
            PseudoNode {
                node: Node::Array,
                folded: false,
                ..
            } if !self.close => {
                out.push(Span::styled("[", style));
            }
            PseudoNode {
                node: Node::Array,
                folded: false,
                ..
            } => {
                out.push(Span::styled("]", style));
            }
            PseudoNode {
                node: Node::Object,
                folded: true,
                ..
            } => {
                out.push(Span::styled("{...}", style));
                if has_comma {
                    out.push(Span::raw(","));
                }
                out.push(Span::styled(
                    format!(" ({} items)", v.children().count()),
                    Style::default().add_modifier(Modifier::DIM),
                ));
            }
            PseudoNode {
                node: Node::Object,
                folded: false,
                ..
            } if !self.close => {
                out.push(Span::styled("{", style));
            }
            PseudoNode {
                node: Node::Object,
                folded: false,
                ..
            } => {
                out.push(Span::styled("}", style));
            }
        }
        self.advance();
        Some(out)
    }
}

fn json_to_text<'a>(
    v: NodeRef<'a, PseudoNode>,
    focus: Option<NodeId>,
) -> impl Iterator<Item = Vec<Span<'a>>> {
    JsonText {
        indent: 0,
        v: Some(v),
        close: false,
        focus,
    }
}

type Screen = AlternateScreen<MouseTerminal<RawTerminal<io::Stdout>>>;

struct App {
    terminal: Terminal<TermionBackend<Screen>>,
    left: View,
    right: Option<View>,
    new_query: Option<String>,
    query: String,
}

struct View {
    scroll: u16,
    values: Vec<Value>,
    content: Vec<Tree<PseudoNode>>,
    focus: Option<(usize, NodeId)>,
}

impl View {
    fn new(values: Vec<Value>) -> Self {
        let content = jsons_to_trees(values.iter());
        let focus = content.get(0).map(|tree| (0, tree.root().id()));
        View {
            scroll: 0,
            values,
            content,
            focus,
        }
    }
    fn render(&self) -> Paragraph {
        let View {
            content,
            focus,
            scroll,
            ..
        } = self;
        let text: Vec<Spans> = content
            .iter()
            .enumerate()
            .flat_map(|(i, tree)| {
                let node_focus =
                    focus.and_then(|(idx, node)| if i == idx { Some(node) } else { None });
                json_to_text(tree.root(), node_focus)
            })
            .map(Spans::from)
            .collect();
        Paragraph::new(text)
            .style(Style::default().fg(Color::White).bg(Color::Black))
            .alignment(Alignment::Left)
            .scroll((*scroll, 0))
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
        let stdout = io::stdout().into_raw_mode()?;
        let stdout = MouseTerminal::from(stdout);
        let stdout = AlternateScreen::from(stdout);
        let backend = TermionBackend::new(stdout);
        let terminal = Terminal::new(backend)?;
        let left = View::new(content);
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
