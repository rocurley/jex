use crate::{
    cursor::{Cursor, Path},
    jq::{jv::JV, run_jq_query, JQ},
};
use serde_json::Deserializer;
use std::{collections::HashSet, io, ops::RangeInclusive, rc::Rc};
use tui::{
    layout::Alignment,
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
