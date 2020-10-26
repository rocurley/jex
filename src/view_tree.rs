use crate::{
    cursor::{Cursor, Path},
    jq::{jv::JV, run_jq_query, JQ},
};
use std::{collections::HashSet, ops::RangeInclusive, rc::Rc};
use tui::{
    layout::Alignment,
    style::{Color, Style},
    text::Spans,
    widgets::Paragraph,
};

#[derive(Debug)]
pub enum View {
    Json(Option<JsonView>),
    Error(Vec<String>),
}

impl View {
    pub fn new(values: Vec<JV>) -> Self {
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

#[derive(Debug)]
pub struct JsonView {
    pub scroll: Cursor,
    pub values: Rc<[JV]>,
    pub cursor: Cursor,
    pub folds: HashSet<(usize, Vec<usize>)>,
}

impl JsonView {
    pub fn new(values: Vec<JV>) -> Option<Self> {
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
