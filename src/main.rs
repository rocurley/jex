use ego_tree::{NodeId, NodeMut, NodeRef, Tree};
use serde_json::json;
use serde_json::value::{Number, Value};
use std::io;
use std::iter::once;
use std::iter::Peekable;
use termion::input::TermRead;
use termion::raw::{IntoRawMode, RawTerminal};
use termion::{event::Key, input::MouseTerminal, screen::AlternateScreen};
use tui::backend::TermionBackend;
use tui::layout::Alignment;
use tui::style::{Color, Style};
use tui::text::{Span, Spans};
use tui::widgets::{Block, Borders, Paragraph, Wrap};
use tui::Terminal;

#[derive(Debug)]
struct PseudoNode {
    node: Node,
    key: Option<String>,
    folded: bool,
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

fn json_to_tree(v: Value) -> Tree<PseudoNode> {
    let mut tree = Tree::new(PseudoNode {
        node: json_to_node(&v),
        key: None,
        folded: false,
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
        Value::Array(_) => Node::Array,
        Value::Object(_) => Node::Object,
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
                    folded: false,
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
                    folded: false,
                });
                append_json_children(child, x);
            }
        }
        _ => {}
    };
}

fn prior_node<T>(n: NodeRef<T>) -> Option<NodeRef<T>> {
    let sib = match n.prev_sibling() {
        None => return n.parent(),
        Some(n) => n,
    };
    Some(sib.last_children().last().unwrap_or(sib))
}

fn next_node<T>(n: NodeRef<T>) -> Option<NodeRef<T>> {
    let child = n.first_child();
    if child.is_some() {
        return child;
    }
    once(n)
        .chain(n.ancestors())
        .filter_map(|n| n.next_sibling())
        .next()
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
                }
            }
            Key::Up => {
                let focus = app.content.get(app.focus).expect("Invalid focus");
                if let Some(prior) = prior_node(focus) {
                    app.focus = prior.id();
                }
            }
            Key::Char('z') => {
                let mut focus = app.content.get_mut(app.focus).expect("Invalid focus");
                let node = focus.value();
                node.folded = !node.folded;
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
        Node::Array if v.value().folded => {
            prefix.push(Span::styled("[...]", style));
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
        Node::Object if v.value().folded => {
            prefix.push(Span::styled("{...}", style));
            Box::new(once(prefix))
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
        let content = json_to_tree(json!({"hello": "world", "array": [1, 2, 3]}));
        let focus = content.root().id();
        Ok(App {
            terminal,
            content,
            focus,
        })
    }
    fn render(&mut self) -> io::Result<()> {
        let App {
            terminal,
            content,
            focus,
        } = self;
        terminal.draw(|f| {
            let size = f.size();
            let text: Vec<Spans> = json_to_text_2(0, content.root(), *focus)
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
