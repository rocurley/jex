use crate::{
    cursor::{Cursor, Path},
    jq::{jv::JV, run_jq_query, JQ},
};
use serde_json::Deserializer;
use std::{collections::HashSet, io, ops::RangeInclusive, rc::Rc};
use tui::{
    layout::Alignment,
    style::{Color, Style},
    text::Spans,
    widgets::Paragraph,
};

// Edit tree requirements
// * Show a parent on the left and a child on the right
// * Children can be modified if they have no children
// * Allow copying descendents onto another root, so you if you want to modify a tree's root you
// can do so by making a new root and then copying over the descendents
// * Views should be named. Root can be the filename, default names can be like parent:0.
// * Select the child, not the parent, so it's unambiguous.
// * There should be a tree viewer on the left (toggleable?) that will let you navigate the tree.
//   * The tree should be presented in a little ascii art tree:
//
//     root
//     ├child
//     │└grandchild
//     └child

pub struct ViewTree {
    view: View,
    children: Vec<(String, ViewTree)>,
}

impl ViewTree {
    pub fn new_from_reader<R: io::Read>(r: R) -> io::Result<Self> {
        let content: Vec<JV> = Deserializer::from_reader(r)
            .into_iter::<JV>()
            .collect::<Result<Vec<JV>, _>>()?;
        let view = View::new(content);
        let mut tree = ViewTree {
            view,
            children: Vec::new(),
        };
        tree.push_trivial_child();
        Ok(tree)
    }
    pub fn push_trivial_child(&mut self) {
        if let View::Json(Some(view)) = &self.view {
            let child = ViewTree {
                view: View::new(view.values.clone()),
                children: Vec::new(),
            };
            self.children.push((".".to_string(), child));
        }
    }
    pub fn index(&self, ix: &ViewTreeIndex) -> Option<(&View, &View, &String)> {
        let mut focus = self;
        let mut path = ix.parent.as_slice();
        while let Some((&i, new_path)) = path.split_first() {
            focus = &focus.children.get(i)?.1;
            path = new_path;
        }
        let (query, child_tree) = focus.children.get(ix.child)?;
        Some((&focus.view, &child_tree.view, query))
    }
    pub fn index_mut(&mut self, ix: &ViewTreeIndex) -> Option<(&mut View, &mut View, &mut String)> {
        let mut focus = self;
        let mut path = ix.parent.as_slice();
        while let Some((&i, new_path)) = path.split_first() {
            focus = &mut focus.children.get_mut(i)?.1;
            path = new_path;
        }
        let (query, child_tree) = focus.children.get_mut(ix.child)?;
        Some((&mut focus.view, &mut child_tree.view, query))
    }
}

pub struct ViewTreeIndex {
    pub parent: Vec<usize>,
    pub child: usize,
}

#[derive(Debug, Clone)]
pub enum View {
    Json(Option<JsonView>),
    Error(Vec<String>),
}

impl View {
    pub fn new<V: Into<Rc<[JV]>>>(values: V) -> Self {
        View::Json(JsonView::new(values))
    }
    pub fn render(&self, line_limit: u16, has_focus: bool) -> Paragraph {
        match self {
            View::Json(Some(json_view)) => json_view.render(line_limit, has_focus),
            View::Json(None) => Paragraph::new(Vec::new()),
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
pub struct JsonView {
    pub scroll: Cursor,
    pub values: Rc<[JV]>,
    pub cursor: Cursor,
    pub folds: HashSet<(usize, Vec<usize>)>,
}

impl JsonView {
    pub fn new<V: Into<Rc<[JV]>>>(values: V) -> Option<Self> {
        let values: Rc<[JV]> = values.into();
        let cursor = Cursor::new(values.clone())?;
        let scroll = Cursor::new(values.clone())?;
        let folds = HashSet::new();
        Some(JsonView {
            scroll,
            values,
            cursor,
            folds,
        })
    }
    fn render(&self, line_limit: u16, has_focus: bool) -> Paragraph {
        let JsonView { cursor, scroll, .. } = self;
        let cursor = if has_focus { Some(cursor) } else { None };
        let text = scroll.clone().render_lines(cursor, &self.folds, line_limit);
        Paragraph::new(text)
            .style(Style::default().fg(Color::White).bg(Color::Black))
            .alignment(Alignment::Left)
        //.wrap(Wrap { trim: false })
    }
    pub fn apply_query(&self, query: &str) -> View {
        match JQ::compile(query) {
            Ok(mut prog) => match run_jq_query(self.values.iter(), &mut prog) {
                Ok(results) => View::Json(JsonView::new(results)),
                Err(err) => View::Error(vec![err]),
            },
            Err(err) => View::Error(err),
        }
    }
    pub fn visible_range(
        &self,
        folds: &HashSet<(usize, Vec<usize>)>,
        line_limit: usize,
    ) -> RangeInclusive<Path> {
        let first = self.scroll.to_path();
        let mut scroll = Cursor::from_path(self.values.clone(), &first);
        for _ in 0..line_limit.saturating_sub(1) {
            scroll.advance(folds);
        }
        let last = scroll.to_path();
        first..=last
    }
    pub fn unfold_around_cursor(&mut self) {
        let mut path = self.cursor.to_path().strip_position();
        while !path.1.is_empty() {
            self.folds.remove(&path);
            path.1.pop();
        }
    }
}
