use crate::{
    cursor::{Cursor, FocusPosition, Path},
    jq::{
        jv::JV,
        query::{run_jq_query, JQ},
    },
};
use serde_json::Deserializer;
use std::{collections::HashSet, io, ops::RangeInclusive, rc::Rc};
use tui::{
    layout::{Alignment, Rect},
    style::{Color, Style},
    text::{Span, Spans},
    widgets::Paragraph,
};

#[derive(Debug, Clone)]
pub struct ViewTree {
    pub view_frame: ViewFrame,
    pub children: Vec<(String, ViewTree)>,
}

#[derive(Debug, Clone)]
pub struct ViewFrame {
    pub view: View,
    pub name: String,
}

impl ViewTree {
    pub fn new_from_reader<R: io::Read>(r: R, name: String) -> io::Result<Self> {
        let content: Vec<JV> = Deserializer::from_reader(r)
            .into_iter::<JV>()
            .collect::<Result<Vec<JV>, _>>()?;
        let view = View::new(content);
        let view_frame = ViewFrame { view, name };
        let mut tree = ViewTree {
            view_frame,
            children: Vec::new(),
        };
        tree.push_trivial_child();
        Ok(tree)
    }
    pub fn push_trivial_child(&mut self) {
        if let View::Json(Some(view)) = &self.view_frame.view {
            let name = "New Query".into();
            let view_frame = ViewFrame {
                view: View::new(view.values.clone()),
                name,
            };
            let child = ViewTree {
                view_frame,
                children: Vec::new(),
            };
            self.children.push((".".to_string(), child));
        }
    }
    pub fn index_tree(&self, mut path: &[usize]) -> Option<&Self> {
        let mut focus = self;
        while let Some((&i, new_path)) = path.split_first() {
            focus = &focus.children.get(i)?.1;
            path = new_path;
        }
        Some(focus)
    }
    pub fn index_tree_mut(&mut self, mut path: &[usize]) -> Option<&mut Self> {
        let mut focus = self;
        while let Some((&i, new_path)) = path.split_first() {
            focus = &mut focus.children.get_mut(i)?.1;
            path = new_path;
        }
        Some(focus)
    }
    pub fn index(&self, ix: &ViewTreeIndex) -> Option<(&ViewFrame, &ViewFrame, &String)> {
        let focus = self.index_tree(&ix.parent)?;
        let (query, child_tree) = focus.children.get(ix.child)?;
        Some((&focus.view_frame, &child_tree.view_frame, query))
    }
    pub fn index_mut(
        &mut self,
        ix: &ViewTreeIndex,
    ) -> Option<(&mut ViewFrame, &mut ViewFrame, &mut String)> {
        let focus = self.index_tree_mut(&ix.parent)?;
        let (query, child_tree) = focus.children.get_mut(ix.child)?;
        Some((&mut focus.view_frame, &mut child_tree.view_frame, query))
    }
    pub fn render_tree(&self, index: &ViewTreeIndex) -> Paragraph {
        let is_parent = index.parent.is_empty();
        let mut spans = vec![render_tree_entry(&self.view_frame.name, is_parent, false).into()];
        for (i, (_, child)) in self.children.iter().enumerate() {
            let end = i == self.children.len() - 1;
            let is_child = is_parent && index.child == i;
            let index = index.borrowed().descend(i);
            render_tree_inner(child, "".into(), end, index, is_child, &mut spans);
        }
        Paragraph::new(spans).style(Style::default().fg(Color::White).bg(Color::Black))
    }
}

fn render_tree_inner<'a, 'b>(
    tree: &'a ViewTree,
    prefix: &str,
    end: bool,
    index: Option<BorrowedViewTreeIndex>,
    is_child: bool,
    out: &mut Vec<Spans<'a>>,
) {
    let is_parent = index.map_or(false, |index| index.parent.is_empty());
    let mid = if end { "└" } else { "├" };
    out.push(
        vec![
            prefix.to_owned().into(),
            mid.into(),
            render_tree_entry(&tree.view_frame.name, is_parent, is_child),
        ]
        .into(),
    );
    let new_prefix = format!("{}{}", prefix, if end { ' ' } else { '│' });
    for (i, (_, child)) in tree.children.iter().enumerate() {
        let end = i == tree.children.len() - 1;
        let is_child = is_parent && index.map_or(false, |index| index.child == i);
        let index = index.and_then(|index| index.descend(i));
        render_tree_inner(child, &new_prefix, end, index, is_child, out);
    }
}

fn render_tree_entry(name: &str, is_parent: bool, is_child: bool) -> Span {
    let style = match (is_parent, is_child) {
        (false, false) => Style::default(),
        (true, false) => Style::default().fg(Color::Blue),
        (false, true) => Style::default().fg(Color::Yellow),
        (true, true) => panic!("Can't be both a parent and a child"),
    };
    Span::styled(name, style)
}

pub struct ViewTreeIndex {
    pub parent: Vec<usize>,
    pub child: usize,
}

impl ViewTreeIndex {
    fn borrowed<'a>(&'a self) -> BorrowedViewTreeIndex<'a> {
        BorrowedViewTreeIndex {
            parent: &self.parent,
            child: self.child,
        }
    }
    pub fn advance(&mut self, views: &ViewTree) -> Option<()> {
        self.advance_inner(views, 0)
    }
    fn advance_inner(&mut self, views: &ViewTree, offset: usize) -> Option<()> {
        match self.parent.get(offset) {
            // we're at the parent
            None => {
                let (_, child) = &views.children[self.child];
                if !child.children.is_empty() {
                    self.parent.push(self.child);
                    self.child = 0;
                    return Some(());
                }
                if self.child == views.children.len() - 1 {
                    None
                } else {
                    self.child += 1;
                    Some(())
                }
            }
            Some(&child_ix) => {
                let (_, child) = &views.children[child_ix];
                if let Some(()) = self.advance_inner(child, offset + 1) {
                    return Some(());
                }
                if child_ix == views.children.len() - 1 {
                    None
                } else {
                    self.child = child_ix + 1;
                    self.parent.truncate(offset);
                    Some(())
                }
            }
        }
    }
    pub fn regress(&mut self) -> Option<()> {
        if self.child == 0 {
            self.child = self.parent.pop()?;
        } else {
            self.child -= 1;
        }
        Some(())
    }
}

#[derive(Clone, Copy)]
struct BorrowedViewTreeIndex<'a> {
    parent: &'a [usize],
    child: usize,
}

impl<'a> BorrowedViewTreeIndex<'a> {
    fn descend(self, ix: usize) -> Option<Self> {
        let (first, rest) = self.parent.split_first()?;
        if *first != ix {
            return None;
        }
        Some(BorrowedViewTreeIndex {
            parent: rest,
            child: self.child,
        })
    }
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
    pub fn render(&self, rect: Rect, has_focus: bool) -> Paragraph {
        match self {
            View::Json(Some(json_view)) => json_view.render(rect, has_focus),
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
    fn render(&self, rect: Rect, has_focus: bool) -> Paragraph {
        let JsonView { cursor, scroll, .. } = self;
        let cursor = if has_focus { Some(cursor) } else { None };
        let text = scroll.clone().render_lines(cursor, &self.folds, rect);
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
    pub fn toggle_fold(&mut self) {
        let path = self.cursor.to_path().strip_position();
        if self.folds.contains(&path) {
            self.folds.remove(&path);
        } else {
            self.folds.insert(path);
            if let FocusPosition::End = self.cursor.focus_position {
                self.cursor.focus_position = FocusPosition::Start;
            }
            if self.scroll.descends_from_or_matches(&self.cursor) {
                self.scroll = self.cursor.clone();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::JsonView;
    use crate::{cursor::Cursor, jq::jv::JV, testing::arb_json};
    use pretty_assertions::assert_eq;
    use proptest::proptest;
    use serde_json::{Deserializer, Value};
    use std::{collections::HashSet, fs, io};
    use tui::layout::Rect;
    fn check_folds(values: Vec<Value>) {
        let jsons: Vec<JV> = values.iter().map(|v| v.into()).collect();
        let mut view = match JsonView::new(jsons) {
            None => return,
            Some(view) => view,
        };
        loop {
            let saved_cursor = view.cursor.clone();
            view.toggle_fold();
            view.toggle_fold();
            // Folding resets you to the top of the fold
            view.cursor = saved_cursor;
            assert_eq!(view.folds, HashSet::new());
            if view.cursor.advance(&view.folds).is_none() {
                break;
            }
        }
    }
    proptest! {
        #[test]
        fn prop_folds(values in proptest::collection::vec(arb_json(), 1..10)) {
            check_folds(values);
        }
    }
    #[test]
    fn unit_folds() {
        let json_path = "example.json";
        let f = fs::File::open(&json_path).unwrap();
        let r = io::BufReader::new(f);
        let jsons: Vec<JV> = Deserializer::from_reader(r)
            .into_iter::<JV>()
            .collect::<Result<Vec<JV>, _>>()
            .unwrap();
        let mut view = JsonView::new(jsons).unwrap();
        view.cursor = Cursor::new_end(view.values.clone()).unwrap();
        view.scroll = view.cursor.clone();
        let line_limit = 20;
        let rect = Rect {
            x: 0,
            y: 0,
            width: 30,
            height: 20,
        };
        for _ in 0..line_limit - 1 {
            view.scroll.regress(&view.folds);
        }
        view.toggle_fold();
        view.render(rect, true);
    }
}
