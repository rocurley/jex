use ego_tree::{NodeId, NodeMut, NodeRef, Tree};
use serde_json::json;
use serde_json::value::{Map, Number, Value};
use std::collections::HashMap;
use std::io;
use std::iter::once;
use std::iter::Peekable;
use termion::input::TermRead;
use termion::raw::{IntoRawMode, RawTerminal};
use termion::{event::Key, input::MouseTerminal, screen::AlternateScreen};
use tui::backend::TermionBackend;
use tui::layout::Alignment;
use tui::layout::{Constraint, Direction, Layout};
use tui::style::{Color, Modifier, Style};
use tui::text::{Span, Spans};
use tui::widgets::{Block, Borders, Clear, Paragraph, Widget, Wrap};
use tui::Terminal;

struct RenderedNode {
    folded: bool,
    focused: bool,
    content: Content,
}

enum Content {
    Null,
    Bool(bool),
    Number(Number),
    String(String),
    Array(Vec<RenderedNode>),
    Object(HashMap<String, RenderedNode>),
}

#[derive(Debug)]
struct PseudoNode {
    node: Node,
    key: Option<String>,
}

#[derive(Debug)]
enum Node {
    Null,
    Bool(bool),
    Number(Number),
    String(String),
    Array,
    Object,
}

impl From<Value> for RenderedNode {
    fn from(v: Value) -> RenderedNode {
        let content = match v {
            Value::Null => Content::Null,
            Value::Bool(b) => Content::Bool(b),
            Value::Number(x) => Content::Number(x),
            Value::String(s) => Content::String(s),
            Value::Array(arr) => Content::Array(arr.into_iter().map(Value::into).collect()),
            Value::Object(obj) => Content::Object(
                obj.into_iter()
                    .map(|(k, v)| (k, RenderedNode::from(v)))
                    .collect(),
            ),
        };
        RenderedNode {
            folded: false,
            focused: false,
            content,
        }
    }
}

fn json_to_tree(v: Value) -> Tree<PseudoNode> {
    let mut tree = Tree::new(PseudoNode {
        node: json_to_node(&v),
        key: None,
    });
    append_json_children(tree.root_mut(), v);
    tree
}

fn json_to_node(v: &Value) -> Node {
    match v {
        Value::Null => Node::Null,
        Value::Bool(b) => Node::Bool(*b),
        Value::Number(x) => Node::Number(x.clone()),
        Value::String(s) => Node::String(s.clone()),
        Value::Array(arr) => Node::Array,
        Value::Object(obj) => Node::Object,
    }
}

fn append_json_children(mut parent: NodeMut<PseudoNode>, v: Value) {
    match v {
        Value::Array(arr) => {
            for x in arr {
                let child_node = json_to_node(&x);
                let child = parent.append(PseudoNode {
                    node: child_node,
                    key: None,
                });
                append_json_children(child, x);
            }
        }
        Value::Object(obj) => {
            for (k, x) in obj {
                let child_node = json_to_node(&x);
                let child = parent.append(PseudoNode {
                    key: Some(k),
                    node: child_node,
                });
                append_json_children(child, x);
            }
        }
        _ => {}
    };
}

fn prior_node<T>(n: NodeRef<T>) -> Option<NodeRef<T>> {
    n.prev_sibling().or_else(|| n.parent())
}

fn next_node<T>(n: NodeRef<T>) -> Option<NodeRef<T>> {
    let child = n.first_child();
    if child.is_some() {
        return child;
    }
    let mut n_option = Some(n);
    while let Some(n) = n_option {
        let sibling = n.next_sibling();
        if sibling.is_some() {
            return sibling;
        }
        n_option = n.parent();
    }
    None
}

fn main() -> Result<(), io::Error> {
    let stdin = io::stdin();
    let mut app = App::new()?;
    app.render()?;
    for c in stdin.keys() {
        match c? {
            Key::Esc => break,
            Key::Down => {
                let focus = app.content.get(app.focus).expect("Invalid focus");
                if let Some(next) = next_node(focus) {
                    app.focus = next.id();
                } else {
                    println!("No next!");
                }
            }
            _ => {}
        }
        app.render()?;
    }
    Ok(())
}

fn json_to_text_2<'a>(
    indent_n: usize,
    v: NodeRef<'a, PseudoNode>,
    focus: NodeId,
) -> Box<dyn Iterator<Item = Vec<Span<'a>>> + 'a> {
    let indent = Span::raw("  ".repeat(indent_n));
    let style = if v.id() == focus {
        Style::default().bg(Color::Blue)
    } else {
        Style::default()
    };
    dbg!(v, style);
    let node = &v.value().node;
    let mut prefix = match &v.value().key {
        None => vec![indent],
        Some(key) => vec![indent, Span::raw(format!("{:?}", key)), Span::raw(" : ")],
    };
    match node {
        Node::Null => {
            prefix.push(Span::styled("null", style));
            Box::new(once(prefix))
        }
        Node::String(s) => {
            prefix.push(Span::styled(format!("{:?}", s), style));
            Box::new(once(prefix))
        }
        Node::Bool(b) => {
            prefix.push(Span::styled(b.to_string(), style));
            Box::new(once(prefix))
        }
        Node::Number(x) => {
            prefix.push(Span::styled(x.to_string(), style));
            Box::new(once(prefix))
        }
        Node::Array => {
            prefix.push(Span::styled("[", style));
            let indent = Span::raw("  ".repeat(indent_n));
            let close = once(vec![indent, Span::styled("]", style)]);
            let values = zip_with_is_last(v.children()).flat_map(move |(v, is_last)| {
                if is_last {
                    json_to_text_2(indent_n + 1, v, focus)
                } else {
                    Box::new(append_comma(json_to_text_2(indent_n + 1, v, focus)))
                }
            });
            Box::new(once(prefix).chain(values).chain(close))
        }
        Node::Object => {
            prefix.push(Span::styled("{", style));
            let indent = Span::raw("  ".repeat(indent_n));
            let close = once(vec![indent, Span::styled("}", style)]);
            let values = zip_with_is_last(v.children()).flat_map(move |(v, is_last)| {
                if is_last {
                    json_to_text_2(indent_n + 1, v, focus)
                } else {
                    Box::new(append_comma(json_to_text_2(indent_n + 1, v, focus)))
                }
            });
            Box::new(once(prefix).chain(values).chain(close))
        }
    }
}

fn json_to_text<'a>(
    indent_n: usize,
    v: &'a RenderedNode,
) -> Box<dyn Iterator<Item = Vec<Span>> + 'a> {
    let indent = Span::raw("  ".repeat(indent_n));
    let style = if v.focused {
        Style::default().bg(Color::Blue)
    } else {
        Style::default()
    };
    match &v.content {
        Content::Null => Box::new(once(vec![indent, Span::styled("null", style)])),
        Content::String(s) => Box::new(once(vec![indent, Span::styled(format!("{:?}", s), style)])),
        Content::Bool(b) => Box::new(once(vec![indent, Span::styled(b.to_string(), style)])),
        Content::Number(x) => Box::new(once(vec![indent, Span::styled(x.to_string(), style)])),
        Content::Array(arr) => {
            let open = once(vec![indent.clone(), Span::styled("[", style)]);
            let close = once(vec![indent, Span::styled("]", style)]);
            let values = zip_with_is_last(arr.iter()).flat_map(move |(v, is_last)| {
                if is_last {
                    json_to_text(indent_n + 1, v)
                } else {
                    Box::new(append_comma(json_to_text(indent_n + 1, v)))
                }
            });
            Box::new(open.chain(values).chain(close))
        }
        Content::Object(obj) => {
            let open = once(vec![indent.clone(), Span::styled("{", style)]);
            let close = once(vec![indent, Span::styled("}", style)]);
            let values_no_commas = obj.iter().map(move |(k, v)| {
                map_first(json_to_text(indent_n + 1, v), move |mut spans| {
                    spans.insert(1, Span::raw(format!("{:?}", k)));
                    spans.insert(2, Span::raw(" : "));
                    spans
                })
            });
            let values = zip_with_is_last(values_no_commas).flat_map(move |(v, is_last)| {
                if is_last {
                    Box::new(v) as Box<dyn Iterator<Item = _>>
                } else {
                    Box::new(append_comma(v))
                }
            });
            Box::new(open.chain(values).chain(close))
        }
    }
}

type Screen = AlternateScreen<MouseTerminal<RawTerminal<io::Stdout>>>;

struct App {
    terminal: Terminal<TermionBackend<Screen>>,
    content: Tree<PseudoNode>,
    focus: NodeId,
}
impl App {
    fn new() -> io::Result<Self> {
        let stdout = io::stdout().into_raw_mode()?;
        let stdout = MouseTerminal::from(stdout);
        let stdout = AlternateScreen::from(stdout);
        let backend = TermionBackend::new(stdout);
        let terminal = Terminal::new(backend)?;
        let mut content = json_to_tree(json!({"hello": "world", "array": [1, 2, 3]}));
        let focus = content.root().id();
        Ok(App {
            terminal,
            content,
            focus,
        })
    }
    fn render(&mut self) -> io::Result<()> {
        let App {
            terminal, content, ..
        } = self;
        terminal.draw(|f| {
            let size = f.size();
            let text: Vec<Spans> = json_to_text_2(0, content.root(), content.root().id())
                .map(Spans::from)
                .collect();
            let block = Block::default().title("Block").borders(Borders::ALL);
            let paragraph = Paragraph::new(text)
                .block(block)
                .style(Style::default().fg(Color::White).bg(Color::Black))
                .alignment(Alignment::Left)
                .wrap(Wrap { trim: false });
            f.render_widget(paragraph, size);
        })
    }
}

fn append_comma<'a, I>(iter: I) -> impl Iterator<Item = Vec<Span<'a>>>
where
    I: Iterator<Item = Vec<Span<'a>>>,
{
    zip_with_is_last(iter).map(|(mut line, is_last)| {
        if is_last {
            line.push(Span::raw(","));
            line
        } else {
            line
        }
    })
}

fn map_first<T, I, F>(iter: I, f: F) -> MapFirst<T, I, F>
where
    I: Iterator<Item = T>,
    F: FnOnce(T) -> T,
{
    MapFirst { iter, f: Some(f) }
}

struct MapFirst<T, I, F>
where
    I: Iterator<Item = T>,
    F: FnOnce(T) -> T,
{
    f: Option<F>,
    iter: I,
}

impl<T, I, F> Iterator for MapFirst<T, I, F>
where
    I: Iterator<Item = T>,
    F: FnOnce(T) -> T,
{
    type Item = T;
    fn next(&mut self) -> Option<T> {
        match self.f.take() {
            None => self.iter.next(),
            Some(f) => self.iter.next().map(f),
        }
    }
}

fn zip_with_is_last<T, I>(iter: I) -> ZipWithIsLast<T, I>
where
    I: Iterator<Item = T>,
{
    ZipWithIsLast {
        iter: iter.peekable(),
    }
}

struct ZipWithIsLast<T, I>
where
    I: Iterator<Item = T>,
{
    iter: Peekable<I>,
}

impl<T, I> Iterator for ZipWithIsLast<T, I>
where
    I: Iterator<Item = T>,
{
    type Item = (T, bool);
    fn next(&mut self) -> Option<(T, bool)> {
        self.iter
            .next()
            .map(|next| (next, self.iter.peek().is_none()))
    }
}
