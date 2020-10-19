use crate::lines::{next_displayable_line_raw, Line, LineContent};
use serde_json::{value::Value, Map};
use tui::{
    style::{Color, Modifier, Style},
    text::{Span, Spans},
};

// In the future, we could store the shadow tree in a vec. This would let us do a AOS -> SOA
// transformation, which in turn would let us replace folded with a bitvector, going from 64 bits
// (padding) to 1.
//
// Another alternative would be to identify each interior node with its left child, which is always
// a leaf (that is to say, ObjectStart or ArrayStart). We could then store interior node
// information (folded and sibling_start_index) in an array indexed by the index of the left child,
// which means we wouldn't need to store the tree structure (children).
//
// Yet another alternative would be to use "fat" indices that effectively serialize the route
// through the tree (ala [1, "key", 7]). This would eliminate the need for sibling_start_index
// entirely. We'd still need to store folding information somewhere, however. Since we wouldn't be
// using integer indices, we couldn't use the sparse array representation, and since we want to
// hold open the option of only using the jv representation, we can't move folded inline.
#[derive(Debug, Clone)]
pub struct Shadow {
    pub folded: bool,
    pub sibling_start_index: usize,
    pub children: Box<[Shadow]>,
}

pub fn construct_shadow_tree(values: &[Value]) -> Shadow {
    let mut i = 0;
    let children = shadow_tree_children(&mut i, values.iter());
    Shadow {
        folded: false,
        sibling_start_index: i,
        children,
    }
}

fn construct_shadow_tree_inner(mut i: usize, value: &Value) -> Option<Shadow> {
    match value {
        Value::Array(arr) => {
            i += 1; // ArrayStart
            let children = shadow_tree_children(&mut i, arr.iter());
            Some(Shadow {
                folded: false,
                sibling_start_index: i + 1, //ArrayEnd
                children,
            })
        }
        Value::Object(obj) => {
            i += 1; // ObjectStart
            let children = shadow_tree_children(&mut i, obj.values());
            Some(Shadow {
                folded: false,
                sibling_start_index: i + 1, //ObjectEnd
                children,
            })
        }
        _ => None,
    }
}

fn shadow_tree_children<'a, I: ExactSizeIterator<Item = &'a Value>>(
    i: &mut usize,
    values: I,
) -> Box<[Shadow]> {
    let mut shadow_children = Vec::with_capacity(values.len());
    for child in values {
        let shadow_child = construct_shadow_tree_inner(*i, child);
        match shadow_child {
            Some(shadow_child) => {
                *i = shadow_child.sibling_start_index;
                shadow_children.push(shadow_child);
            }
            None => *i += 1,
        };
    }
    shadow_children.into()
}

pub fn index(i: usize, shadow_node: &Shadow, values: &[Value]) -> Option<Line> {
    let indent = 0;
    let current_index = 0;
    let key: Option<&str> = None;
    let node = Node::Top(values);
    if i >= shadow_node.sibling_start_index {
        return None;
    }
    Some(index_inner(
        i,
        shadow_node,
        node,
        current_index,
        key,
        indent,
        false,
    ))
}

enum Node<'a> {
    Top(&'a [Value]),
    Value(&'a Value),
}

fn index_inner(
    ix: usize,
    shadow_node: &Shadow,
    node: Node,
    mut current_index: usize,
    key: Option<&str>,
    indent: u8,
    final_comma: bool,
) -> Line {
    assert!(ix < shadow_node.sibling_start_index);
    assert!(current_index <= ix);
    let zipped_children: Box<dyn ExactSizeIterator<Item = _>> = match node {
        Node::Top(arr) => Box::new(zip_array_shadow(shadow_node, arr)),
        Node::Value(Value::Array(arr)) => {
            if ix == current_index {
                return Line {
                    key: key.map(|key| key.into()),
                    folded: shadow_node.folded,
                    indent,
                    content: LineContent::ArrayStart(
                        shadow_node.sibling_start_index - current_index - 2,
                    ),
                    comma: false,
                };
            }
            if ix == shadow_node.sibling_start_index - 1 {
                return Line {
                    key: None,
                    folded: shadow_node.folded,
                    indent,
                    content: LineContent::ArrayEnd(
                        shadow_node.sibling_start_index - current_index - 2,
                    ),
                    comma: final_comma,
                };
            }
            current_index += 1; // Skip ArrayStart
            Box::new(zip_array_shadow(shadow_node, arr))
        }
        Node::Value(Value::Object(obj)) => {
            if ix == current_index {
                return Line {
                    key: key.map(|key| key.into()),
                    folded: shadow_node.folded,
                    indent,
                    content: LineContent::ObjectStart(
                        shadow_node.sibling_start_index - current_index - 2,
                    ),
                    comma: false,
                };
            }
            if ix == shadow_node.sibling_start_index - 1 {
                return Line {
                    key: None,
                    folded: shadow_node.folded,
                    indent,
                    content: LineContent::ObjectEnd(
                        shadow_node.sibling_start_index - current_index - 2,
                    ),
                    comma: final_comma,
                };
            }
            current_index += 1; // Skip ObjectStart
            Box::new(zip_map_shadow(shadow_node, obj))
        }
        Node::Value(_) => panic!("index_inner should only be called on a non-leaf node"),
    };
    let new_indent = match node {
        Node::Top(_) => indent,
        Node::Value(_) => indent + 1,
    };
    let len = zipped_children.len();
    for (i, (shadow_child, key, child)) in zipped_children.enumerate() {
        let child_has_comma = if let Node::Top(_) = node {
            false
        } else {
            i != len - 1
        };
        match shadow_child {
            Some(shadow_child) => {
                if ix < shadow_child.sibling_start_index {
                    return index_inner(
                        ix,
                        shadow_child,
                        Node::Value(child),
                        current_index,
                        key,
                        new_indent,
                        child_has_comma,
                    );
                } else {
                    current_index = shadow_child.sibling_start_index;
                }
            }
            None => {
                if ix == current_index {
                    return leaf_to_line(new_indent, key, child, child_has_comma);
                }
                current_index += 1;
            }
        }
    }
    panic!("Couldn't find a child for this index: is the shadow tree malformed?");
}

pub fn next_displayable_line(i: usize, shadow: &Shadow, values: &[Value]) -> Option<usize> {
    let line = index(i, shadow, values)?;
    let new_i = next_displayable_line_raw(i, &line);
    if new_i >= shadow.sibling_start_index {
        None
    } else {
        Some(new_i)
    }
}

pub fn prior_displayable_line(i: usize, shadow: &Shadow, values: &[Value]) -> Option<usize> {
    let i = i.checked_sub(1)?;
    let line = index(i, shadow, values)?;
    match &line.content {
        LineContent::ArrayEnd(lines_skipped) | LineContent::ObjectEnd(lines_skipped) => {
            let matching_i = i - 1 - lines_skipped;
            let matching_line = index(matching_i, shadow, values).unwrap();
            // TODO: apply folded to both ends of array/object so we don't need to do this.
            if matching_line.folded {
                Some(matching_i)
            } else {
                Some(i)
            }
        }
        _ => Some(i),
    }
}

fn leaf_to_line(indent: u8, key: Option<&str>, node: &Value, comma: bool) -> Line {
    let content = match node {
        Value::Null => LineContent::Null,
        Value::Bool(b) => LineContent::Bool(*b),
        Value::Number(x) => LineContent::Number(x.clone()),
        Value::String(s) => LineContent::String(s.as_str().into()),
        Value::Array(_) => panic!("Called leaf_to_line on an array"),
        Value::Object(_) => panic!("Called leaf_to_line on an object"),
    };
    Line {
        content,
        indent,
        key: key.map(|key| key.into()),
        folded: false,
        comma,
    }
}

fn zip_array_shadow<'a: 'c, 'b: 'c, 'c>(
    shadow_node: &'a Shadow,
    children: &'b [Value],
) -> impl ExactSizeIterator<Item = (Option<&'a Shadow>, Option<&'b str>, &'b Value)> + 'c {
    let mut shadow_children = shadow_node.children.iter();
    children.iter().map(move |child| match child {
        Value::Array(_) | Value::Object(_) => (shadow_children.next(), None, child),
        _ => (None, None, child),
    })
}

fn zip_map_shadow<'a: 'c, 'b: 'c, 'c>(
    shadow_node: &'a Shadow,
    children: &'b Map<String, Value>,
) -> impl ExactSizeIterator<Item = (Option<&'a Shadow>, Option<&'b str>, &'b Value)> + 'c {
    let mut shadow_children = shadow_node.children.iter();
    children.iter().map(move |(key, child)| match child {
        Value::Array(_) | Value::Object(_) => (shadow_children.next(), Some(key.as_str()), child),
        _ => (None, Some(key.as_str()), child),
    })
}

pub fn render_line(i: usize, cursor: Option<usize>, line: Line) -> Spans<'static> {
    let indent_span = Span::raw("  ".repeat(line.indent as usize));
    let mut out = match &line.key {
        Some(key) => vec![
            indent_span,
            Span::raw(format!("{:?}", key)),
            Span::raw(" : "),
        ],
        _ => vec![indent_span],
    };
    let style = if Some(i) == cursor {
        Style::default().bg(Color::Blue)
    } else {
        Style::default()
    };
    match line {
        Line {
            content: LineContent::Null,
            ..
        } => {
            out.push(Span::styled("null", style));
            if line.comma {
                out.push(Span::raw(","));
            }
        }
        Line {
            content: LineContent::String(s),
            ..
        } => {
            out.push(Span::styled(format!("{:?}", s), style));
            if line.comma {
                out.push(Span::raw(","));
            }
        }
        Line {
            content: LineContent::Bool(b),
            ..
        } => {
            out.push(Span::styled(b.to_string(), style));
            if line.comma {
                out.push(Span::raw(","));
            }
        }
        Line {
            content: LineContent::Number(x),
            ..
        } => {
            out.push(Span::styled(x.to_string(), style));
            if line.comma {
                out.push(Span::raw(","));
            }
        }
        Line {
            content: LineContent::ArrayStart(skipped_lines),
            folded: true,
            ..
        } => {
            out.push(Span::styled("[...]", style));
            if line.comma {
                out.push(Span::raw(","));
            }
            out.push(Span::styled(
                format!(" ({} lines)", skipped_lines),
                Style::default().add_modifier(Modifier::DIM),
            ));
        }
        Line {
            content: LineContent::ArrayEnd(_),
            folded: true,
            ..
        } => {
            panic!("Attempted to print close of folded array");
        }
        Line {
            content: LineContent::ArrayStart(_),
            folded: false,
            ..
        } => {
            out.push(Span::styled("[", style));
        }
        Line {
            content: LineContent::ArrayEnd(_),
            folded: false,
            ..
        } => {
            out.push(Span::styled("]", style));
            if line.comma {
                out.push(Span::raw(","));
            }
        }
        Line {
            content: LineContent::ObjectStart(skipped_lines),
            folded: true,
            ..
        } => {
            out.push(Span::styled("{...}", style));
            if line.comma {
                out.push(Span::raw(","));
            }
            out.push(Span::styled(
                format!(" ({} lines)", skipped_lines),
                Style::default().add_modifier(Modifier::DIM),
            ));
        }
        Line {
            content: LineContent::ObjectEnd(_),
            folded: true,
            ..
        } => {
            panic!("Attempted to print close of folded array");
        }
        Line {
            content: LineContent::ObjectStart(_),
            folded: false,
            ..
        } => {
            out.push(Span::styled("{", style));
        }
        Line {
            content: LineContent::ObjectEnd(_),
            folded: false,
            ..
        } => {
            out.push(Span::styled("}", style));
            if line.comma {
                out.push(Span::raw(","));
            }
        }
    };
    Spans::from(out)
}
pub fn render_lines<'a>(
    scroll: usize,
    line_limit: u16,
    cursor: Option<usize>,
    shadow_tree: &'a Shadow,
    values: &'a [Value],
) -> Vec<Spans<'a>> {
    renderable_lines(scroll, shadow_tree, values)
        .take(line_limit as usize)
        .map(|(i, line)| render_line(i, cursor, line))
        .collect()
}
pub fn renderable_lines<'a>(
    scroll: usize,
    shadow_tree: &'a Shadow,
    values: &'a [Value],
) -> impl Iterator<Item = (usize, Line)> + 'a {
    RenderableLines {
        i: scroll,
        shadow_tree,
        values,
    }
}

struct RenderableLines<'a> {
    i: usize,
    shadow_tree: &'a Shadow,
    values: &'a [Value],
}
impl<'a> Iterator for RenderableLines<'a> {
    type Item = (usize, Line);
    fn next(&mut self) -> Option<(usize, Line)> {
        let i = self.i;
        let out = index(self.i, self.shadow_tree, self.values)?;
        self.i = next_displayable_line_raw(self.i, &out);
        Some((i, out))
    }
}

pub mod mutable {
    use super::{zip_array_shadow, zip_map_shadow, Shadow};
    use serde_json::Value;
    pub fn index_shadow<'a, 'b>(
        i: usize,
        shadow_node: &'a mut Shadow,
        values: &'b [Value],
    ) -> Option<(usize, &'a mut Shadow)> {
        let current_index = 0;
        let node = Node::Top(values);
        if i >= shadow_node.sibling_start_index {
            return None;
        }
        Some(index_inner(i, shadow_node, node, current_index))
    }

    #[derive(Debug)]
    enum Node<'a> {
        Top(&'a [Value]),
        Value(&'a Value),
    }

    fn index_inner<'a, 'b>(
        ix: usize,
        shadow_node: &'a mut Shadow,
        node: Node<'b>,
        mut current_index: usize,
    ) -> (usize, &'a mut Shadow) {
        assert!(ix < shadow_node.sibling_start_index);
        assert!(current_index <= ix);
        let current_node_cannonical_index = current_index;
        let mut zipped_children: Box<dyn ExactSizeIterator<Item = _>> = match node {
            Node::Top(arr) => Box::new(zip_array_shadow(shadow_node, arr)),
            Node::Value(Value::Array(arr)) => {
                if ix == current_index {
                    return (current_node_cannonical_index, shadow_node);
                }
                if ix == shadow_node.sibling_start_index - 1 {
                    return (current_node_cannonical_index, shadow_node);
                }
                current_index += 1; // Skip ArrayStart
                Box::new(zip_array_shadow(shadow_node, arr))
            }
            Node::Value(Value::Object(obj)) => {
                if ix == current_index {
                    return (current_node_cannonical_index, shadow_node);
                }
                if ix == shadow_node.sibling_start_index - 1 {
                    return (current_node_cannonical_index, shadow_node);
                }
                current_index += 1; // Skip ObjectStart
                Box::new(zip_map_shadow(shadow_node, obj))
            }
            Node::Value(_) => panic!("index_inner should only be called on a non-leaf node"),
        };
        // This is pretty clunky. If we do the obvious thing of making a zipped_children iterate
        // over `&mut Shadow`s, the borrow checker will unify the lifetime of the child with 'a,
        // and so won't let us return the parent (since the parent is mutably borrowed through the
        // lifetime of this function. Instead, we find the index of the child we want using an
        // immutable borrow, then re-borrow it mutable after the loop.
        let mut current_child_index = 0;
        let (selected_child_index, selected_child) = loop {
            let (shadow_child, _, child) = zipped_children
                .next()
                .expect("Couldn't find a child for this index: is the shadow tree malformed?");
            match shadow_child {
                Some(shadow_child) => {
                    if ix < shadow_child.sibling_start_index {
                        break (Some(current_child_index), child);
                    } else {
                        current_index = shadow_child.sibling_start_index;
                        current_child_index += 1;
                    }
                }
                None => {
                    if ix == current_index {
                        break (None, child);
                    }
                    current_index += 1;
                }
            }
        };
        drop(zipped_children);
        match selected_child_index {
            None => (current_node_cannonical_index, shadow_node),
            Some(i) => index_inner(
                ix,
                &mut shadow_node.children[i],
                Node::Value(selected_child),
                current_index,
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{construct_shadow_tree, renderable_lines};
    use crate::{
        lines::Line,
        testing::{arb_json, json_to_lines},
    };
    use pretty_assertions::assert_eq;
    use proptest::proptest;
    proptest! {
        #[test]
        fn prop_lines(values in proptest::collection::vec(arb_json(), 1..10)) {
            let shadow_tree = construct_shadow_tree(&values);
            let actual_lines : Vec<Line> = renderable_lines(0, &shadow_tree, &values).map(|(_, line)| line).collect();
            let expected_lines = json_to_lines(values.iter());
            assert_eq!(actual_lines, expected_lines);
        }
    }
}