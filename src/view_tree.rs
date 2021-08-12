use crate::{
    cursor::{FocusPosition, GlobalCursor, GlobalPath, LeafCursor, ValuePath},
    jq::{
        jv::JV,
        query::{run_jq_query, JQ},
    },
    layout::JexLayout,
    lines::LineCursor,
};
use log::trace;
use serde_json::Deserializer;
use std::{collections::HashSet, io, io::Write, ops::RangeInclusive, rc::Rc};
use tui::{
    layout::{Alignment, Rect},
    style::{Color, Style},
    text::{Span, Spans},
    widgets::{Block, Borders, Paragraph},
};

#[derive(Debug, Clone)]
pub struct ViewForest {
    pub trees: Vec<ViewTree>,
}

impl ViewForest {
    pub fn index(&self, ix: &ViewForestIndex) -> Option<(&ViewFrame, &ViewFrame, &String)> {
        let tree = self.trees.get(ix.tree)?;
        tree.index(&ix.within_tree)
    }
    pub fn index_mut(
        &mut self,
        ix: &ViewForestIndex,
    ) -> Option<(&mut ViewFrame, &mut ViewFrame, &mut String)> {
        let tree = self.trees.get_mut(ix.tree)?;
        tree.index_mut(&ix.within_tree)
    }
    pub fn render_tree(&self, index: &ViewForestIndex) -> Paragraph {
        let mut spans = Vec::new();
        for (i, tree) in self.trees.iter().enumerate() {
            let tree_index = if i == index.tree {
                Some(index.within_tree.borrowed())
            } else {
                None
            };
            render_tree_inner(
                tree,
                "",
                i == self.trees.len() - 1,
                tree_index,
                false,
                &mut spans,
            )
        }
        Paragraph::new(spans).style(Style::default().fg(Color::White).bg(Color::Black))
    }
}

#[derive(Debug, Clone)]
pub struct ViewTree {
    pub view_frame: ViewFrame,
    // (query, tree)
    pub children: Vec<(String, ViewTree)>,
}

#[derive(Debug, Clone)]
pub struct ViewFrame {
    pub view: View,
    pub name: String,
}

impl ViewTree {
    pub fn new_from_reader<R: io::Read>(r: R, name: String, layout: JexLayout) -> io::Result<Self> {
        let content: Vec<JV> = Deserializer::from_reader(r)
            .into_iter::<JV>()
            .collect::<Result<Vec<JV>, _>>()?;
        let view = View::new(content, layout.left);
        let view_frame = ViewFrame { view, name };
        let mut tree = ViewTree {
            view_frame,
            children: Vec::new(),
        };
        tree.push_trivial_child(layout.right);
        Ok(tree)
    }
    pub fn push_trivial_child(&mut self, target_view_rect: Rect) {
        if let View::Json(Some(view)) = &self.view_frame.view {
            let name = "New Query".into();
            let view_frame = ViewFrame {
                view: View::new(view.values.clone(), target_view_rect),
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

#[derive(Debug, Clone)]
pub struct ViewForestIndex {
    pub tree: usize,
    pub within_tree: ViewTreeIndex,
}

impl ViewForestIndex {
    pub fn advance(&mut self, forrest: &ViewForest) -> Option<()> {
        if let Some(()) = self.within_tree.advance(&forrest.trees[self.tree]) {
            return Some(());
        }
        if self.tree == forrest.trees.len() - 1 {
            return None;
        }
        self.tree += 1;
        self.within_tree = ViewTreeIndex {
            parent: Vec::new(),
            child: 0,
        };
        Some(())
    }
    pub fn regress(&mut self, forrest: &ViewForest) -> Option<()> {
        if let Some(()) = self.within_tree.regress() {
            return Some(());
        }
        if self.tree == 0 {
            return None;
        }
        self.tree -= 1;
        self.within_tree = ViewTreeIndex::new_at_end(&forrest.trees[self.tree]);
        Some(())
    }
}

#[derive(Debug, Clone)]
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
    // TODO: this isn't the inverse of advance, which is weird
    pub fn regress(&mut self) -> Option<()> {
        if self.child == 0 {
            self.child = self.parent.pop()?;
        } else {
            self.child -= 1;
        }
        Some(())
    }
    pub fn new_at_end(mut tree: &ViewTree) -> Self {
        let mut out = ViewTreeIndex {
            parent: Vec::new(),
            child: tree.children.len() - 1,
        };
        tree = &tree.children.last().unwrap().1;
        while let Some(last_child) = tree.children.last() {
            out.parent.push(out.child);
            out.child = tree.children.len() - 1;
            tree = &last_child.1;
        }
        out
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
    pub fn new<V: Into<Rc<[JV]>>>(values: V, view_rect: Rect) -> Self {
        let json_rect = Block::default().borders(Borders::ALL).inner(view_rect);
        View::Json(JsonView::new(values, json_rect))
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
    pub fn resize_to(&mut self, view_rect: Rect) {
        match self {
            View::Json(Some(v)) => {
                let json_rect = Block::default().borders(Borders::ALL).inner(view_rect);
                v.resize_to(json_rect);
            }
            _ => {}
        }
    }
}

#[derive(Debug, Clone)]
pub struct JsonView {
    pub scroll: GlobalCursor,
    pub values: Rc<[JV]>,
    pub cursor: LeafCursor,
    pub folds: HashSet<(usize, Vec<usize>)>,
    pub rect: Rect,
}

impl JsonView {
    pub fn new<V: Into<Rc<[JV]>>>(values: V, rect: Rect) -> Option<Self> {
        let values: Rc<[JV]> = values.into();
        let cursor = LeafCursor::new(values.clone())?;
        let folds = HashSet::new();
        let scroll = GlobalCursor::new(values.clone(), rect.width, &folds)?;
        Some(JsonView {
            scroll,
            values,
            cursor,
            folds,
            rect,
        })
    }
    fn render(&self, rect: Rect, has_focus: bool) -> Paragraph {
        trace!("Rendering started: target rect {:?}", rect);
        let JsonView { cursor, scroll, .. } = self;
        let cursor = if has_focus { Some(cursor) } else { None };
        let text = scroll.clone().render_lines(cursor, &self.folds, rect);
        trace!("Rendering complete");
        Paragraph::new(text)
            .style(Style::default().fg(Color::White).bg(Color::Black))
            .alignment(Alignment::Left)
        //.wrap(Wrap { trim: false })
    }
    pub fn apply_query(&self, query: &str, target_view_rect: Rect) -> View {
        let target_json_rect = Block::default()
            .borders(Borders::ALL)
            .inner(target_view_rect);
        match JQ::compile(query) {
            Ok(mut prog) => match run_jq_query(self.values.iter(), &mut prog) {
                Ok(results) => View::Json(JsonView::new(results, target_json_rect)),
                Err(err) => View::Error(vec![err]),
            },
            Err(err) => View::Error(err),
        }
    }
    pub fn visible_range(&self, folds: &HashSet<(usize, Vec<usize>)>) -> GlobalPathRange {
        let mut scroll = self.scroll.clone();
        let start = scroll.to_path();
        let mut end_is_line_end = scroll.at_line_end();
        for _ in 1..self.rect.height {
            if let None = scroll.advance(folds, self.rect.width) {
                break;
            };
            end_is_line_end = scroll.at_line_end();
        }
        let end = scroll.to_path();
        GlobalPathRange {
            start,
            end,
            end_is_last_line: end_is_line_end,
        }
    }
    pub fn page_down(&mut self) {
        for _ in 1..self.rect.height {
            if let None = self.scroll.advance(&self.folds, self.rect.width) {
                break;
            };
        }
        for _ in 1..self.rect.height {
            if let None = self.cursor.advance(&self.folds) {
                break;
            };
        }
    }
    pub fn page_up(&mut self) {
        for _ in 1..self.rect.height {
            if let None = self.scroll.regress(&self.folds, self.rect.width) {
                break;
            };
        }
        for _ in 1..self.rect.height {
            if let None = self.cursor.regress(&self.folds) {
                break;
            };
        }
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
            if self
                .scroll
                .value_cursor
                .descends_from_or_matches(&self.cursor)
            {
                let line = self.cursor.current_line(&self.folds, self.rect.width);
                let line_cursor = LineCursor::new_at_start(line.render(), self.rect.width);
                self.scroll = GlobalCursor {
                    value_cursor: self.cursor.clone(),
                    // Note: this is okay because you can only fold objects and arrays
                    line_cursor,
                };
            }
        }
    }
    pub fn advance_cursor(&mut self) {
        let visible_range = self.visible_range(&self.folds);
        if !visible_range.contains_value_end(&self.cursor.to_path()) {
            self.scroll.advance(&self.folds, self.rect.width);
            return;
        }
        self.cursor.advance(&self.folds);
        if !visible_range.contains_value(&self.cursor.to_path()) {
            self.scroll.advance(&self.folds, self.rect.width);
        }
    }
    pub fn regress_cursor(&mut self) {
        let visible_range = self.visible_range(&self.folds);
        if !visible_range.contains_value_start(&self.cursor.to_path()) {
            self.scroll.regress(&self.folds, self.rect.width);
            return;
        }
        self.cursor.regress(&self.folds);
        if !visible_range.contains_value(&self.cursor.to_path()) {
            self.scroll.regress(&self.folds, self.rect.width);
        }
    }
    pub fn resize_to(&mut self, json_rect: Rect) {
        self.rect = json_rect;
        self.scroll.resize_to(json_rect);
        while self.cursor.to_path() < **self.visible_range(&self.folds).value_range().start() {
            self.scroll.regress(&self.folds, self.rect.width);
        }
        while self.cursor.to_path() > **self.visible_range(&self.folds).value_range().end() {
            self.scroll.advance(&self.folds, self.rect.width);
        }
    }
    pub fn save_to(&self, path: &str) -> std::io::Result<()> {
        let mut file = std::fs::File::create(path)?;
        for (i, v) in self.values.iter().enumerate() {
            if i != 0 {
                write!(file, "\n")?;
            }
            serde_json::to_writer_pretty(&mut file, v)?;
        }
        Ok(())
    }
}

#[derive(Debug)]
pub struct GlobalPathRange {
    start: GlobalPath,
    end: GlobalPath,
    end_is_last_line: bool,
}

impl GlobalPathRange {
    pub fn value_range(&self) -> RangeInclusive<&ValuePath> {
        &self.start.value_path..=&self.end.value_path
    }
    pub fn contains_value(&self, path: &ValuePath) -> bool {
        self.value_range().contains(&path)
    }
    pub fn contains_value_start(&self, path: &ValuePath) -> bool {
        if !self.contains_value(path) {
            return false;
        }
        if *path != self.start.value_path {
            return true;
        }
        self.start.current_line == 0
    }
    pub fn contains_value_end(&self, path: &ValuePath) -> bool {
        if !self.contains_value(path) {
            return false;
        }
        if *path != self.end.value_path {
            return true;
        }
        self.end_is_last_line
    }
}

#[cfg(test)]
mod tests {
    use super::JsonView;
    use crate::{cursor::GlobalCursor, jq::jv::JV, testing::arb_json};
    use pretty_assertions::assert_eq;
    use proptest::proptest;
    use serde_json::{Deserializer, Value};
    use std::{collections::HashSet, fs, io};
    use tui::layout::Rect;
    const DUMMY_RECT: Rect = Rect {
        x: 1,
        y: 1,
        width: 135,
        height: 70,
    };
    const TINY_RECT: Rect = Rect {
        x: 1,
        y: 1,
        width: 15,
        height: 20,
    };
    fn check_folds(values: Vec<Value>) {
        let jsons: Vec<JV> = values.iter().map(|v| v.into()).collect();
        let mut view = match JsonView::new(jsons, DUMMY_RECT) {
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
        let json_path = "testdata/example.json";
        let f = fs::File::open(&json_path).unwrap();
        let r = io::BufReader::new(f);
        let jsons: Vec<JV> = Deserializer::from_reader(r)
            .into_iter::<JV>()
            .collect::<Result<Vec<JV>, _>>()
            .unwrap();
        let mut view = JsonView::new(jsons, DUMMY_RECT).unwrap();
        view.scroll =
            GlobalCursor::new_end(view.values.clone(), DUMMY_RECT.width, &HashSet::new()).unwrap();
        view.cursor = view.scroll.value_cursor.clone();
        let line_limit = 20;
        let rect = Rect {
            x: 0,
            y: 0,
            width: 30,
            height: 20,
        };
        for _ in 0..line_limit - 1 {
            view.scroll.regress(&view.folds, DUMMY_RECT.width);
        }
        view.toggle_fold();
        view.render(rect, true);
    }
    #[test]
    fn unit_scroll_render() {
        simplelog::TestLogger::init(log::LevelFilter::Trace, Default::default()).unwrap();
        let json_path = "testdata/example.json";
        let f = fs::File::open(&json_path).unwrap();
        let r = io::BufReader::new(f);
        let jsons: Vec<JV> = Deserializer::from_reader(r)
            .into_iter::<JV>()
            .collect::<Result<Vec<JV>, _>>()
            .unwrap();
        let mut view = JsonView::new(jsons.clone(), DUMMY_RECT).unwrap();
        let right_rect = Rect {
            x: 138,
            ..DUMMY_RECT
        };
        let right_view = JsonView::new(jsons, right_rect).unwrap();
        let folds = HashSet::new();
        view.render(DUMMY_RECT, true);
        right_view.render(right_rect, true);
        while let Some(()) = view.scroll.advance(&folds, DUMMY_RECT.width) {
            view.render(DUMMY_RECT, true);
            right_view.render(right_rect, true);
        }
    }
    #[test]
    fn unit_render_small() {
        let json_path = "testdata/example.json";
        let f = fs::File::open(&json_path).unwrap();
        let r = io::BufReader::new(f);
        let jsons: Vec<JV> = Deserializer::from_reader(r)
            .into_iter::<JV>()
            .collect::<Result<Vec<JV>, _>>()
            .unwrap();
        let view = JsonView::new(jsons, TINY_RECT).unwrap();
        view.render(TINY_RECT, true);
    }
}
