use crate::{
    cursor::GlobalCursor,
    layout::{self, JexLayout},
    view_tree::{View, ViewFrame, ViewTree, ViewTreeIndex},
};
use log::debug;
use regex::Regex;
use std::{default::Default, io};
use tui::{
    layout::{Alignment, Rect},
    text::Text,
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
    Frame,
};

const README: &str = include_str!("../README.md");

pub struct App {
    pub views: ViewTree,
    pub index: ViewTreeIndex,
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
        let views = ViewTree::new_from_reader(r, name, layout)?;
        let index = ViewTreeIndex {
            parent: Vec::new(),
            child: 0,
        };
        let app = App {
            views,
            index,
            focus: Focus::Left,
            search_re: None,
            show_tree: false,
            flash: None,
        };
        Ok(app)
    }
    fn current_views(&self) -> (&ViewFrame, &ViewFrame, &String) {
        self.views
            .index(&self.index)
            .expect("App index invalidated")
    }
    pub fn current_views_mut(&mut self) -> (&mut ViewFrame, &mut ViewFrame, &mut String) {
        self.views
            .index_mut(&self.index)
            .expect("App index invalidated")
    }
    pub fn focused_view(&self) -> &ViewFrame {
        let (left, right, _) = self.current_views();
        match self.focus {
            Focus::Left => left,
            Focus::Right => right,
        }
    }
    pub fn focused_view_mut(&mut self) -> &mut ViewFrame {
        let focus = self.focus;
        let (left, right, _) = self.current_views_mut();
        match focus {
            Focus::Left => left,
            Focus::Right => right,
        }
    }
    pub fn recompute_right(&mut self, right_rect: Rect) {
        let (left, right, query) = self.current_views_mut();
        match &mut left.view {
            View::Json(Some(left)) => {
                right.view = left.apply_query(query, right_rect);
            }
            View::Json(None) | View::Error(_) => {
                right.view = View::Json(None);
            }
        }
    }
    pub fn render<B: tui::backend::Backend>(
        &self,
        mode: AppRenderMode,
    ) -> impl FnMut(&mut Frame<B>) + '_ {
        let App { focus, .. } = self;
        let (left, right, query) = self.current_views();
        move |f| {
            let size = f.size();
            let layout = JexLayout::new(size, self.show_tree);
            let left_block = Block::default()
                .title(left.name.to_owned())
                .borders(Borders::ALL);
            let left_paragraph = left
                .view
                .render(left_block.inner(layout.left), *focus == Focus::Left)
                .block(left_block);
            f.render_widget(left_paragraph, layout.left);
            let right_block = Block::default()
                .title(right.name.to_owned())
                .borders(Borders::ALL);
            let right_paragraph = right
                .view
                .render(right_block.inner(layout.right), *focus == Focus::Right)
                .block(right_block);
            f.render_widget(right_paragraph, layout.right);
            if let Some(tree_rect) = layout.tree {
                let tree_block = Block::default().borders(Borders::ALL);
                f.render_widget(
                    self.views.render_tree(&self.index).block(tree_block),
                    tree_rect,
                );
            }
            match mode {
                AppRenderMode::Normal => {
                    let query = Paragraph::new(query.as_str())
                        .alignment(Alignment::Left)
                        .wrap(Wrap { trim: false });
                    f.render_widget(query, layout.query);
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
        let re = if let Some(re) = &self.search_re {
            re
        } else {
            return;
        };
        let (left, right, _) = self
            .views
            .index_mut(&self.index)
            .expect("App index invalidated");
        let view = match self.focus {
            Focus::Left => left,
            Focus::Right => right,
        };
        let view = if let View::Json(Some(view)) = &mut view.view {
            view
        } else {
            return;
        };
        let search_hit = if reverse {
            view.cursor.clone().search_back(re)
        } else {
            view.cursor.clone().search(re)
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
        let (left, right, _) = self.current_views_mut();
        left.view.resize_to(layout.left);
        right.view.resize_to(layout.right);
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
}
