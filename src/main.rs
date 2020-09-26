use ego_tree::{NodeId, NodeMut, NodeRef, Tree};
use serde_json::json;
use serde_json::value::{Number, Value};
use std::io;
use std::iter::once;
use termion::input::TermRead;
use termion::raw::{IntoRawMode, RawTerminal};
use termion::{event::Key, input::MouseTerminal, screen::AlternateScreen};
use tui::backend::TermionBackend;
use tui::layout::Alignment;
use tui::style::{Color, Modifier, Style};
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

fn jsons_to_trees<I: Iterator<Item = Value>>(vs: I) -> Vec<Tree<PseudoNode>> {
    vs.map(|v| {
        let mut tree = Tree::new(PseudoNode {
            node: json_to_node(&v),
            key: None,
            folded: false,
        });
        append_json_children(tree.root_mut(), v);
        tree
    })
    .collect()
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

fn prior_node(n: NodeRef<PseudoNode>) -> Option<NodeRef<PseudoNode>> {
    let sib = match n.prev_sibling() {
        None => return n.parent(),
        Some(n) => n,
    };
    let mut last = sib;
    for n in once(sib).chain(sib.last_children()) {
        if n.value().folded {
            return Some(n);
        }
        last = n;
    }
    Some(last)
}

fn next_node(n: NodeRef<PseudoNode>) -> Option<NodeRef<PseudoNode>> {
    if !n.value().folded {
        let child = n.first_child();
        if child.is_some() {
            return child;
        }
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
                let focus_node = app.content[app.focus.0]
                    .get(app.focus.1)
                    .expect("Invalid focus");
                match next_node(focus_node) {
                    Some(next) => app.focus.1 = next.id(),
                    None if app.focus.0 + 1 == app.content.len() => {}
                    None => {
                        app.focus = (app.focus.0 + 1, app.content[app.focus.0 + 1].root().id());
                    }
                }
            }
            Key::Up => {
                let focus_node = app.content[app.focus.0]
                    .get(app.focus.1)
                    .expect("Invalid focus");
                match prior_node(focus_node) {
                    Some(prior) => app.focus.1 = prior.id(),
                    None if app.focus.0 == 0 => {}
                    None => {
                        let root = app.content[app.focus.0 - 1].root();
                        let focus_node = root.last_children().last().unwrap_or(root);
                        app.focus = (app.focus.0 - 1, focus_node.id());
                    }
                }
            }
            Key::Char('z') => {
                let mut focus_node = app.content[app.focus.0]
                    .get_mut(app.focus.1)
                    .expect("Invalid focus");
                let node = focus_node.value();
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
    focus: Option<NodeId>,
) -> Box<dyn Iterator<Item = Vec<Span<'a>>> + 'a> {
    let indent = Span::raw("  ".repeat(indent_n));
    let style = if Some(v.id()) == focus {
        Style::default().bg(Color::Blue)
    } else {
        Style::default()
    };
    let node = &v.value().node;
    let mut prefix = match &v.value().key {
        None => vec![indent],
        Some(key) => vec![indent, Span::raw(format!("{:?}", key)), Span::raw(" : ")],
    };
    let has_comma = v.next_sibling().is_some();
    match node {
        Node::Null => {
            prefix.push(Span::styled("null", style));
            if has_comma {
                prefix.push(Span::raw(","));
            }
            Box::new(once(prefix))
        }
        Node::String(s) => {
            prefix.push(Span::styled(format!("{:?}", s), style));
            if has_comma {
                prefix.push(Span::raw(","));
            }
            Box::new(once(prefix))
        }
        Node::Bool(b) => {
            prefix.push(Span::styled(b.to_string(), style));
            if has_comma {
                prefix.push(Span::raw(","));
            }
            Box::new(once(prefix))
        }
        Node::Number(x) => {
            prefix.push(Span::styled(x.to_string(), style));
            if has_comma {
                prefix.push(Span::raw(","));
            }
            Box::new(once(prefix))
        }
        Node::Array if v.value().folded => {
            prefix.push(Span::styled("[...]", style));
            if has_comma {
                prefix.push(Span::raw(","));
            }
            prefix.push(Span::styled(
                format!(" ({} items)", v.children().count()),
                Style::default().add_modifier(Modifier::DIM),
            ));
            Box::new(once(prefix))
        }
        Node::Array => {
            prefix.push(Span::styled("[", style));
            let indent = Span::raw("  ".repeat(indent_n));
            let mut close = vec![indent, Span::styled("]", style)];
            if has_comma {
                close.push(Span::raw(","));
            }
            let values = v.children().flat_map(move |v| {
                if v.next_sibling().is_none() {
                    json_to_text_2(indent_n + 1, v, focus)
                } else {
                    Box::new(json_to_text_2(indent_n + 1, v, focus))
                }
            });
            Box::new(once(prefix).chain(values).chain(once(close)))
        }
        Node::Object if v.value().folded => {
            prefix.push(Span::styled("{...}", style));
            if has_comma {
                prefix.push(Span::raw(","));
            }
            prefix.push(Span::styled(
                format!(" ({} items)", v.children().count()),
                Style::default().add_modifier(Modifier::DIM),
            ));
            Box::new(once(prefix))
        }
        Node::Object => {
            prefix.push(Span::styled("{", style));
            let indent = Span::raw("  ".repeat(indent_n));
            let mut close = vec![indent, Span::styled("}", style)];
            if has_comma {
                close.push(Span::raw(","));
            }
            let values = v.children().flat_map(move |v| {
                if v.next_sibling().is_none() {
                    json_to_text_2(indent_n + 1, v, focus)
                } else {
                    Box::new(json_to_text_2(indent_n + 1, v, focus))
                }
            });
            Box::new(once(prefix).chain(values).chain(once(close)))
        }
    }
}

type Screen = AlternateScreen<MouseTerminal<RawTerminal<io::Stdout>>>;

struct App {
    terminal: Terminal<TermionBackend<Screen>>,
    content: Vec<Tree<PseudoNode>>,
    focus: (usize, NodeId),
}
impl App {
    fn new() -> io::Result<Self> {
        let stdout = io::stdout().into_raw_mode()?;
        let stdout = MouseTerminal::from(stdout);
        let stdout = AlternateScreen::from(stdout);
        let backend = TermionBackend::new(stdout);
        let terminal = Terminal::new(backend)?;
        let content = jsons_to_trees(
            vec![
                json!({"hello": "world", "array": [1, 2, 3]}),
                json!({"hello": "world", "array": [1, 2, 3]}),
                json!({"hello": "world", "array": [1, 2, 3]}),
                json!({"hello": "world", "array": [1, 2, 3]}),
                json!({"hello": "world", "array": [1, 2, 3]}),
                json!({"hello": "world", "array": [1, 2, 3]}),
                json!({"hello": "world", "array": [1, 2, 3]}),
                json!({"hello": "world", "array": [1, 2, 3]}),
                json!({"hello": "world", "array": [1, 2, 3]}),
                json!({"hello": "world", "array": [1, 2, 3]}),
                json!({"hello": "world", "array": [1, 2, 3]}),
                json!({"hello": "world", "array": [1, 2, 3]}),
                json!({"hello": "world", "array": [1, 2, 3]}),
                json!({"hello": "world", "array": [1, 2, 3]}),
                json!({"hello": "world", "array": [1, 2, 3]}),
                json!({"hello": "world", "array": [1, 2, 3]}),
            ]
            .into_iter(),
        );
        let focus = (0, content[0].root().id());
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
            let text: Vec<Spans> = content
                .iter()
                .enumerate()
                .flat_map(|(i, tree)| {
                    let node_focus = if i == focus.0 { Some(focus.1) } else { None };
                    json_to_text_2(0, tree.root(), node_focus)
                })
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
