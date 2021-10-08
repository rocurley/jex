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

#[derive(Debug, Clone, Copy)]
pub enum ViewWithParent<'a> {
    Root {
        frame: &'a NamedView,
    },
    Child {
        frame: &'a NamedView,
        parent: &'a NamedView,
        query: &'a String,
    },
}

impl<'a> ViewWithParent<'a> {
    pub fn frame(self) -> &'a NamedView {
        match self {
            ViewWithParent::Root { frame } | ViewWithParent::Child { frame, .. } => frame,
        }
    }
}

#[derive(Debug)]
pub enum ViewWithParentMut<'a> {
    Root {
        frame: &'a mut NamedView,
    },
    Child {
        frame: &'a mut NamedView,
        parent: &'a mut NamedView,
        query: &'a mut String,
    },
}

impl<'a: 'b, 'b> ViewWithParentMut<'a> {
    pub fn frame(&'b mut self) -> &'b mut NamedView {
        match self {
            ViewWithParentMut::Root { frame } | ViewWithParentMut::Child { frame, .. } => frame,
        }
    }
    pub fn take_frame(self) -> &'a mut NamedView {
        match self {
            ViewWithParentMut::Root { frame } | ViewWithParentMut::Child { frame, .. } => frame,
        }
    }
}

impl ViewForest {
    pub fn index(&self, ix: &ViewForestIndex) -> Option<ViewWithParent> {
        let tree = self.trees.get(ix.tree)?;
        tree.index(&ix.within_tree)
    }
    pub fn index_mut(&mut self, ix: &ViewForestIndex) -> Option<ViewWithParentMut> {
        let tree = self.trees.get_mut(ix.tree)?;
        tree.index_mut(&ix.within_tree)
    }
    pub fn render_tree(
        &self,
        left_index: &ViewForestIndex,
        right_index: &ViewForestIndex,
    ) -> Paragraph {
        let mut spans = Vec::new();
        for (i, tree) in self.trees.iter().enumerate() {
            let left_tree_index = if i == left_index.tree {
                Some(left_index.within_tree.borrowed())
            } else {
                None
            };
            let right_tree_index = if i == right_index.tree {
                Some(left_index.within_tree.borrowed())
            } else {
                None
            };
            render_tree_inner(
                tree,
                "",
                i == self.trees.len() - 1,
                left_tree_index,
                right_tree_index,
                &mut spans,
            )
        }
        Paragraph::new(spans).style(Style::default().fg(Color::White).bg(Color::Black))
    }
}

#[derive(Debug, Clone)]
pub struct ViewTree {
    pub view_frame: NamedView,
    // (query, tree)
    pub children: Vec<(String, ViewTree)>,
}

#[derive(Debug, Clone)]
pub struct NamedView {
    pub view: View,
    pub name: String,
}

impl ViewTree {
    pub fn new_from_reader<R: io::Read>(r: R, name: String, layout: JexLayout) -> io::Result<Self> {
        let content: Vec<JV> = Deserializer::from_reader(r)
            .into_iter::<JV>()
            .collect::<Result<Vec<JV>, _>>()?;
        let view = View::new(content, layout.left);
        let view_frame = NamedView { view, name };
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
            let view_frame = NamedView {
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
    pub fn index(&self, ix: &ViewTreeIndex) -> Option<ViewWithParent> {
        let mut focus = self;
        let mut path: &[_] = &*&ix.path;
        let mut out = ViewWithParent::Root {
            frame: &focus.view_frame,
        };
        while let Some((&i, new_path)) = path.split_first() {
            let (query, new_focus) = focus.children.get(i)?;
            focus = new_focus;
            path = new_path;
            out = ViewWithParent::Child {
                query,
                frame: &focus.view_frame,
                parent: out.frame(),
            };
        }
        Some(out)
    }
    pub fn index_mut(&mut self, ix: &ViewTreeIndex) -> Option<ViewWithParentMut> {
        let mut focus = self;
        let mut path: &[_] = &*&ix.path;
        let mut out = Some(ViewWithParentMut::Root {
            frame: &mut focus.view_frame,
        });
        while let Some((&i, new_path)) = path.split_first() {
            let (query, new_focus) = focus.children.get_mut(i)?;
            focus = new_focus;
            path = new_path;
            let parent = out.take().unwrap().take_frame();
            out = Some(ViewWithParentMut::Child {
                query,
                frame: &mut focus.view_frame,
                parent,
            });
        }
        out
    }
}

fn render_tree_inner<'a, 'b>(
    tree: &'a ViewTree,
    prefix: &str,
    end: bool,
    left_index: Option<BorrowedViewTreeIndex>,
    right_index: Option<BorrowedViewTreeIndex>,
    out: &mut Vec<Spans<'a>>,
) {
    let is_left = left_index.map_or(false, |index| index.parent.is_empty());
    let is_right = right_index.map_or(false, |index| index.parent.is_empty());
    let mid = if end { "└" } else { "├" };
    out.push(
        vec![
            prefix.to_owned().into(),
            mid.into(),
            render_tree_entry(&tree.view_frame.name, is_left, is_right),
        ]
        .into(),
    );
    let new_prefix = format!("{}{}", prefix, if end { ' ' } else { '│' });
    for (i, (_, child)) in tree.children.iter().enumerate() {
        let end = i == tree.children.len() - 1;
        let left_index = left_index.and_then(|index| index.descend(i));
        let right_index = right_index.and_then(|index| index.descend(i));
        render_tree_inner(child, &new_prefix, end, left_index, right_index, out);
    }
}

fn render_tree_entry(name: &str, is_parent: bool, is_child: bool) -> Span {
    match (is_parent, is_child) {
        (false, false) => Span::raw(name),
        (true, false) => Span::styled(format!("(L) {}", name), Style::default().fg(Color::Blue)),
        (false, true) => Span::styled(format!("(R) {}", name), Style::default().fg(Color::Yellow)),
        (true, true) => panic!("Can't be both a parent and a child"),
    }
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
        self.within_tree = ViewTreeIndex { path: Vec::new() };
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
    pub path: Vec<usize>,
}

impl ViewTreeIndex {
    fn borrowed<'a>(&'a self) -> BorrowedViewTreeIndex<'a> {
        BorrowedViewTreeIndex { parent: &self.path }
    }
    pub fn advance(&mut self, views: &ViewTree) -> Option<()> {
        self.advance_inner(views, 0)
    }
    fn advance_inner(&mut self, views: &ViewTree, offset: usize) -> Option<()> {
        match self.path.get(offset) {
            None => {
                // We've arrived at the node we're pointing at. Descend into its children if possible.
                if !views.children.is_empty() {
                    self.path.push(0);
                    Some(())
                } else {
                    None
                }
            }
            Some(&child_ix) => {
                let child = &views.children[child_ix].1;
                match self.advance_inner(child, offset + 1) {
                    Some(()) => Some(()), // child advanced
                    None => {
                        let new_child_ix = child_ix + 1;
                        if new_child_ix == views.children.len() {
                            None
                        } else {
                            self.path.truncate(offset);
                            self.path.push(new_child_ix);
                            Some(())
                        }
                    }
                }
            }
        }
    }
    pub fn regress(&mut self) -> Option<()> {
        while let Some(last) = self.path.last_mut() {
            if *last > 0 {
                *last -= 1;
                return Some(());
            }
            self.path.pop();
        }
        None
    }
    pub fn new_at_end(mut tree: &ViewTree) -> Self {
        let mut out = ViewTreeIndex { path: Vec::new() };
        tree = &tree.children.last().unwrap().1;
        while let Some(last_child) = tree.children.last() {
            out.path.push(tree.children.len() - 1);
            tree = &last_child.1;
        }
        out
    }
}

#[derive(Clone, Copy)]
struct BorrowedViewTreeIndex<'a> {
    parent: &'a [usize],
}

impl<'a> BorrowedViewTreeIndex<'a> {
    fn descend(self, ix: usize) -> Option<Self> {
        let (first, rest) = self.parent.split_first()?;
        if *first != ix {
            return None;
        }
        Some(BorrowedViewTreeIndex { parent: rest })
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
