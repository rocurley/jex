use ego_tree::{NodeId, NodeMut, NodeRef, Tree};
use serde_json::json;
use serde_json::value::{Number, Value};
use std::io;
use std::iter::once;
use termion::input::TermRead;
use termion::raw::{IntoRawMode, RawTerminal};
use termion::{event::Key, input::MouseTerminal, screen::AlternateScreen};
use tui::backend::TermionBackend;
use tui::layout::{Alignment, Constraint, Direction, Layout};
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
                            let root = view.content[focus.0 - 1].root();
                            let focus_node = root.last_children().last().unwrap_or(root);
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
            _ => {}
        }
        app.render()?;
    }
    Ok(())
}

fn json_to_text<'a>(
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
            let values = v
                .children()
                .flat_map(move |v| json_to_text(indent_n + 1, v, focus));
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
            let values = v
                .children()
                .flat_map(move |v| json_to_text(indent_n + 1, v, focus));
            Box::new(once(prefix).chain(values).chain(once(close)))
        }
    }
}

type Screen = AlternateScreen<MouseTerminal<RawTerminal<io::Stdout>>>;

struct App {
    terminal: Terminal<TermionBackend<Screen>>,
    left: View,
    right: View,
}

struct View {
    scroll: u16,
    content: Vec<Tree<PseudoNode>>,
    focus: Option<(usize, NodeId)>,
}

impl View {
    fn render(&self) -> Paragraph {
        let View {
            content,
            focus,
            scroll,
        } = self;
        let text: Vec<Spans> = content
            .iter()
            .enumerate()
            .flat_map(|(i, tree)| {
                let node_focus =
                    focus.and_then(|(idx, node)| if i == idx { Some(node) } else { None });
                json_to_text(0, tree.root(), node_focus)
            })
            .map(Spans::from)
            .collect();
        Paragraph::new(text)
            .style(Style::default().fg(Color::White).bg(Color::Black))
            .alignment(Alignment::Left)
            .scroll((*scroll, 0))
            .wrap(Wrap { trim: false })
    }
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
        let focus = Some((0, content[0].root().id()));
        Ok(App {
            terminal,
            left: View {
                content,
                focus,
                scroll: 0,
            },
            right: View {
                content: Vec::new(),
                focus: None,
                scroll: 0,
            },
        })
    }
    fn render(&mut self) -> io::Result<()> {
        let App {
            terminal,
            left,
            right,
        } = self;
        terminal.draw(|f| {
            let size = f.size();
            let chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Ratio(1, 2), Constraint::Ratio(1, 2)].as_ref())
                .split(size);
            let left_block = Block::default().title("Left").borders(Borders::ALL);
            let left_paragraph = left.render().block(left_block);
            f.render_widget(left_paragraph, chunks[0]);
            let right_block = Block::default().title("Right").borders(Borders::ALL);
            let right_paragraph = right.render().block(right_block);
            f.render_widget(right_paragraph, chunks[1]);
        })
    }
}
