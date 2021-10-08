use crate::{
    cursor::GlobalCursor,
    layout::{self, JexLayout},
    view_tree::{
        View, ViewForest, ViewForestIndex, ViewTree, ViewTreeIndex, ViewWithParent,
        ViewWithParentMut,
    },
};
use log::debug;
use regex::Regex;
use std::{default::Default, fs, io};
use tui::{
    layout::{Alignment, Rect},
    text::Text,
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
    Frame,
};

const README: &str = include_str!("../README.md");

pub struct App {
    pub views: ViewForest,
    pub left_index: ViewForestIndex,
    pub right_index: ViewForestIndex,
    pub focus: Focus,
    pub search_re: Option<Regex>,
    pub show_tree: bool,
    pub flash: Option<Flash>,
}

pub struct Flash {
    pub paragraph: Paragraph<'static>,
    pub scroll: u16,
}

pub enum AppRenderMode {
    Normal,
    InputEditor,
}

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum Focus {
    Left,
    Right,
}

impl Focus {
    pub fn swap(self) -> Self {
        match self {
            Focus::Left => Focus::Right,
            Focus::Right => Focus::Left,
        }
    }
}

impl App {
    pub fn new<R: io::Read>(r: R, name: String, layout: JexLayout) -> io::Result<Self> {
        let views = ViewForest {
            trees: vec![ViewTree::new_from_reader(r, name, layout)?],
        };
        let left_index = ViewForestIndex {
            tree: 0,
            within_tree: ViewTreeIndex { path: Vec::new() },
        };
        let right_index = ViewForestIndex {
            tree: 0,
            within_tree: ViewTreeIndex { path: vec![0] },
        };
        let app = App {
            views,
            left_index,
            right_index,
            focus: Focus::Left,
            search_re: None,
            show_tree: false,
            flash: None,
        };
        Ok(app)
    }
    fn current_views(&self) -> (ViewWithParent, ViewWithParent) {
        let left = self
            .views
            .index(&self.left_index)
            .expect("App index invalidated");
        let right = self
            .views
            .index(&self.right_index)
            .expect("App index invalidated");
        (left, right)
    }
    pub fn focused_view(&self) -> ViewWithParent {
        let (left, right) = self.current_views();
        match self.focus {
            Focus::Left => left,
            Focus::Right => right,
        }
    }
    pub fn left_view_mut(&mut self) -> ViewWithParentMut {
        self.views
            .index_mut(&self.left_index)
            .expect("App index invalidated")
    }
    pub fn right_view_mut(&mut self) -> ViewWithParentMut {
        self.views
            .index_mut(&self.right_index)
            .expect("App index invalidated")
    }
    pub fn focused_view_mut(&mut self) -> ViewWithParentMut {
        match self.focus {
            Focus::Left => self.left_view_mut(),
            Focus::Right => self.right_view_mut(),
        }
    }
    pub fn focused_query_mut(&mut self) -> Option<&mut String> {
        match self.focused_view_mut() {
            ViewWithParentMut::Root { .. } => None,
            ViewWithParentMut::Child { query, .. } => Some(query),
        }
    }
    pub fn recompute_focused_view(&mut self, focused_rect: Rect) {
        match self.focused_view_mut() {
            ViewWithParentMut::Root { .. } => panic!("Can't recompute root node"),
            ViewWithParentMut::Child {
                parent,
                query,
                frame,
            } => match &parent.view {
                View::Json(Some(left)) => {
                    frame.view = left.apply_query(query, focused_rect);
                }
                View::Json(None) | View::Error(_) => {
                    frame.view = View::Json(None);
                }
            },
        }
    }
    pub fn render<B: tui::backend::Backend>(
        &self,
        mode: AppRenderMode,
    ) -> impl FnMut(&mut Frame<B>) + '_ {
        let App { focus, .. } = self;
        let (left, right) = self.current_views();
        move |f| {
            let size = f.size();
            let layout = JexLayout::new(size, self.show_tree);
            let left_block = Block::default()
                .title(left.frame().name.to_owned())
                .borders(Borders::ALL);
            let left_paragraph = left
                .frame()
                .view
                .render(left_block.inner(layout.left), *focus == Focus::Left)
                .block(left_block);
            f.render_widget(left_paragraph, layout.left);
            let right_block = Block::default()
                .title(right.frame().name.to_owned())
                .borders(Borders::ALL);
            let right_paragraph = right
                .frame()
                .view
                .render(right_block.inner(layout.right), *focus == Focus::Right)
                .block(right_block);
            f.render_widget(right_paragraph, layout.right);
            if let Some(tree_rect) = layout.tree {
                let tree_block = Block::default().borders(Borders::ALL);
                f.render_widget(
                    self.views
                        .render_tree(&self.left_index, &self.right_index)
                        .block(tree_block),
                    tree_rect,
                );
            }
            match mode {
                AppRenderMode::Normal => {
                    let focused_view = match self.focus {
                        Focus::Left => left,
                        Focus::Right => right,
                    };
                    match focused_view {
                        ViewWithParent::Root { .. } => {
                            let placeholder = Paragraph::new("Root Node")
                                .alignment(Alignment::Left)
                                .wrap(Wrap { trim: false });
                            f.render_widget(placeholder, layout.query);
                        }
                        ViewWithParent::Child { query, .. } => {
                            let query = Paragraph::new(query.as_str())
                                .alignment(Alignment::Left)
                                .wrap(Wrap { trim: false });
                            f.render_widget(query, layout.query);
                        }
                    }
                }
                AppRenderMode::InputEditor => {
                    f.set_cursor(0, layout.query.y);
                }
            }
            if let Some(flash) = self.flash.as_ref() {
                let area = layout::flash(size);
                f.render_widget(Clear, area);
                let block = Block::default()
                    .title("Press ESC to close popup")
                    .borders(Borders::ALL);
                f.render_widget(
                    flash
                        .paragraph
                        .clone()
                        .scroll((flash.scroll, 0))
                        .block(block),
                    area,
                );
            }
        }
    }
    pub fn search(&mut self, reverse: bool) {
        let re = if let Some(re) = self.search_re.clone() {
            re
        } else {
            return;
        };
        let mut view_with_parents = self.focused_view_mut();
        let view_frame = view_with_parents.frame();
        let view = if let View::Json(Some(view)) = &mut view_frame.view {
            view
        } else {
            return;
        };
        let search_hit = if reverse {
            view.cursor.clone().search_back(&re)
        } else {
            view.cursor.clone().search(&re)
        };
        if let Some(search_hit) = search_hit {
            view.cursor = search_hit;
        } else {
            return;
        };
        view.unfold_around_cursor();
        if !view
            .visible_range(&view.folds)
            .contains_value(&view.cursor.to_path())
        {
            view.scroll = GlobalCursor::new(view.values.clone(), view.rect.width, &view.folds)
                .expect("values should still exist");
        }
    }
    pub fn resize(&mut self, layout: JexLayout) {
        debug!("Resizing to new layout: {:?}", layout);
        self.left_view_mut().frame().view.resize_to(layout.left);
        self.right_view_mut().frame().view.resize_to(layout.right);
    }
    pub fn set_flash(&mut self, s: String) {
        self.flash = Some(Flash {
            paragraph: Paragraph::new(Text::from(s)).wrap(Wrap { trim: false }),
            scroll: 0,
        });
    }
    pub fn show_help(&mut self) {
        let controls = README
            .rsplit("<!-- START CONTROLS POPUP -->\n")
            .next()
            .unwrap()
            .split("<!-- END CONTROLS POPUP -->")
            .next()
            .unwrap();
        self.set_flash(controls.to_string());
    }
    pub fn open_file(
        &mut self,
        path: String,
        layout: JexLayout,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let f = fs::File::open(&path)?;
        let r = io::BufReader::new(f);
        let new_tree = ViewTree::new_from_reader(r, path, layout)?;
        self.views.trees.push(new_tree);
        self.left_index = ViewForestIndex {
            tree: self.views.trees.len() - 1,
            within_tree: ViewTreeIndex { path: Vec::new() },
        };
        Ok(())
    }
}
