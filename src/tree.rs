use ego_tree::{NodeMut, NodeRef, Tree};
use serde_json::value::Number;
use serde_json::value::Value;
use std::iter::once;

#[derive(Debug, Clone)]
pub struct PseudoNode {
    pub node: Node,
    pub key: Option<String>,
    pub folded: bool,
}

#[derive(Debug, Clone)]
pub enum Node {
    Null,
    Bool(bool),
    Number(Number),
    String(String),
    Array,
    Object,
}

pub fn jsons_to_trees<'a, I: Iterator<Item = &'a Value>>(vs: I) -> Vec<Tree<PseudoNode>> {
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

fn append_json_children(mut parent: NodeMut<PseudoNode>, v: &Value) {
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
                    key: Some(k.clone()),
                    node: child_node,
                    folded: false,
                });
                append_json_children(child, x);
            }
        }
        _ => {}
    };
}

pub fn prior_node(n: NodeRef<PseudoNode>) -> Option<NodeRef<PseudoNode>> {
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

pub fn next_node(n: NodeRef<PseudoNode>) -> Option<NodeRef<PseudoNode>> {
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

pub fn last_node(tree: &Tree<PseudoNode>) -> NodeRef<PseudoNode> {
    let root = tree.root();
    let mut last = root;
    for n in once(root).chain(root.last_children()) {
        if n.value().folded {
            return n;
        }
        last = n;
    }
    last
}

#[cfg(test)]
mod tests {
    use super::{jsons_to_trees, last_node, next_node, prior_node, PseudoNode};
    use ego_tree::{iter::Edge, Tree};
    use proptest::collection;
    use proptest::{prelude::*, proptest};
    use serde_json::value::Value;
    use std::iter::once;
    fn arb_json() -> impl Strategy<Value = Value> {
        let leaf = prop_oneof![
            Just(Value::Null),
            any::<bool>().prop_map(Value::Bool),
            any::<f64>().prop_map(|f| f.into()),
            ".*".prop_map(Value::String),
        ];
        leaf.prop_recursive(
            8,   // 8 levels deep
            256, // Shoot for maximum size of 256 nodes
            10,  // We put up to 10 items per collection
            |inner| {
                prop_oneof![
                    // Take the inner strategy and make the two recursive cases.
                    prop::collection::vec(inner.clone(), 0..10).prop_map(Value::Array),
                    prop::collection::hash_map(".*", inner, 0..10)
                        .prop_map(|m| { Value::Object(m.into_iter().collect()) }),
                ]
            },
        )
    }
    fn arb_folded_tree() -> impl Strategy<Value = Tree<PseudoNode>> {
        arb_json().prop_flat_map(|json| {
            let tree = jsons_to_trees(once(&json)).pop().unwrap();
            let root = tree.root();
            let ids: Vec<_> = root
                .traverse()
                .map(|edge| match edge {
                    Edge::Open(n) | Edge::Close(n) => n.id(),
                })
                .collect();
            collection::vec(any::<bool>(), ids.len()).prop_map(move |folds| {
                let mut tree = tree.clone();
                for (id, fold) in ids.iter().zip(folds) {
                    tree.get_mut(*id).unwrap().value().folded = fold;
                }
                tree
            })
        })
    }
    proptest! {
        #[test]
        fn test_prev_next(tree in arb_folded_tree()) {
            let mut node = tree.root();
            while let Some(next) = next_node(node) {
                assert_eq!(Some(node), prior_node(next));
                node = next;
            }
            while let Some(prior) = prior_node(node) {
                assert_eq!(Some(node), next_node(prior));
                node = prior;
            }
        }
        #[test]
        fn test_last_node(tree in arb_folded_tree()) {
            let last = last_node(&tree);
            assert_eq!(next_node(last), None);
        }
    }
}
