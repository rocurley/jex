use crate::{
    jq::jv::{JVArray, JVObject, OwnedObjectIterator, JV},
    lines::{escaped_str, Line, LineContent, LineCursor},
};
use regex::Regex;
use std::{borrow::Cow, cmp::Ordering, collections::HashSet, fmt, rc::Rc};
use tui::{
    layout::Rect,
    text::{Span, Spans},
};

// Requirements:
// * Produce the current line
// * Step forward
// * (Optionally, for searching): Step backwards
// * Can be "dehydrated" into something hashable for storing folds (other metadata?)

#[derive(PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Clone, Copy)]
pub enum FocusPosition {
    Start,
    Value,
    End,
}

impl FocusPosition {
    pub fn starting(json: &JV) -> Self {
        match json {
            JV::Array(_) | JV::Object(_) => FocusPosition::Start,
            _ => FocusPosition::Value,
        }
    }
    pub fn ending(json: &JV) -> Self {
        match json {
            JV::Array(_) | JV::Object(_) => FocusPosition::End,
            _ => FocusPosition::Value,
        }
    }
}

#[derive(Clone)]
pub enum CursorFrame {
    Array {
        index: usize,
        json: JVArray,
    },
    Object {
        index: usize,
        key: String,
        json: JVObject,
        iterator: OwnedObjectIterator,
    },
}

impl fmt::Debug for CursorFrame {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        match self {
            CursorFrame::Array { index, json } => fmt
                .debug_struct("Array")
                .field("index", index)
                .field("json", json)
                .finish(),
            CursorFrame::Object {
                index,
                key,
                json,
                iterator: _,
            } => fmt
                .debug_struct("Object")
                .field("index", index)
                .field("key", key)
                .field("json", json)
                .finish(),
        }
    }
}

impl PartialEq for CursorFrame {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (
                CursorFrame::Array { index, json },
                CursorFrame::Array {
                    index: other_index,
                    json: other_json,
                },
            ) => (index == other_index && json == other_json),
            (
                CursorFrame::Object {
                    index, key, json, ..
                },
                CursorFrame::Object {
                    index: other_index,
                    key: other_key,
                    json: other_json,
                    ..
                },
            ) => (index == other_index && json == other_json && key == other_key),
            _ => false,
        }
    }
}
impl Eq for CursorFrame {}

fn open_container(json: JV) -> (Option<CursorFrame>, JV, FocusPosition) {
    match json {
        JV::Array(arr) => {
            let mut iterator = Box::new(arr.clone().into_iter());
            match iterator.next() {
                None => (None, arr.into(), FocusPosition::End),
                Some(child) => {
                    let focus_position = FocusPosition::starting(&child);
                    (
                        Some(CursorFrame::Array {
                            index: 0,
                            json: arr,
                        }),
                        child,
                        focus_position,
                    )
                }
            }
        }
        JV::Object(obj) => {
            let mut iterator = obj.clone().into_iter();
            match iterator.next() {
                None => (None, obj.into(), FocusPosition::End),
                Some((key, child)) => {
                    let focus_position = FocusPosition::starting(&child);
                    (
                        Some(CursorFrame::Object {
                            index: 0,
                            json: obj,
                            key,
                            iterator,
                        }),
                        child,
                        focus_position,
                    )
                }
            }
        }
        _ => panic!("Can't make a cursor frame from a leaf json"),
    }
}

fn open_container_end(json: JV) -> (Option<CursorFrame>, JV, FocusPosition) {
    match json {
        JV::Array(arr) => {
            if arr.is_empty() {
                (None, arr.into(), FocusPosition::Start)
            } else {
                let index = arr.len() - 1;
                let child = arr.get(index).expect("Array should not be empty here");
                let focus_position = FocusPosition::ending(&child);
                (
                    Some(CursorFrame::Array {
                        index: index as usize,
                        json: arr,
                    }),
                    child,
                    focus_position,
                )
            }
        }
        JV::Object(obj) => {
            let iterator = Box::new(obj.clone().into_iter());
            match iterator.last() {
                None => (None, obj.into(), FocusPosition::Start),
                Some((key, child)) => {
                    let index = obj.len() as usize - 1;
                    let focus_position = FocusPosition::ending(&child);
                    (
                        Some(CursorFrame::Object {
                            index,
                            json: obj.clone(),
                            key,
                            iterator: obj.into_empty_iter(),
                        }),
                        child,
                        focus_position,
                    )
                }
            }
        }
        _ => panic!("Can't make a cursor frame from a leaf json"),
    }
}

impl CursorFrame {
    pub fn index(&self) -> usize {
        match self {
            CursorFrame::Array { index, .. } => *index as usize,
            CursorFrame::Object { index, .. } => *index as usize,
        }
    }
    fn advance(self) -> (Option<Self>, JV, FocusPosition) {
        use CursorFrame::*;
        match self {
            Array { index, json } => match json.get(index as i32 + 1) {
                None => (None, json.into(), FocusPosition::End),
                Some(child) => {
                    let focus_position = FocusPosition::starting(&child);
                    (
                        Some(Array {
                            index: index + 1,
                            json,
                        }),
                        child,
                        focus_position,
                    )
                }
            },
            Object {
                index,
                json,
                mut iterator,
                ..
            } => match iterator.next() {
                None => (None, json.into(), FocusPosition::End),
                Some((key, child)) => {
                    let focus_position = FocusPosition::starting(&child);
                    (
                        Some(Object {
                            index: index + 1,
                            key,
                            json,
                            iterator,
                        }),
                        child,
                        focus_position,
                    )
                }
            },
        }
    }
    fn regress(self) -> (Option<Self>, JV, FocusPosition) {
        use CursorFrame::*;
        match self {
            Array { index, json } => match index.checked_sub(1) {
                None => (None, json.into(), FocusPosition::Start),
                Some(index) => {
                    let child = json
                        .get(index as i32)
                        .expect("Stepped back and didn't find a child");
                    let focus_position = FocusPosition::ending(&child);
                    (Some(Array { index, json }), child, focus_position)
                }
            },
            Object {
                index,
                json,
                iterator: _,
                ..
            } => match index.checked_sub(1) {
                None => (None, json.into(), FocusPosition::Start),
                Some(index) => {
                    let mut iterator = json.clone().into_iter();
                    let (key, child) = iterator
                        .nth(index)
                        .expect("Stepped back and didn't find a child");
                    let focus_position = FocusPosition::ending(&child);
                    (
                        Some(Object {
                            index,
                            key,
                            json,
                            iterator,
                        }),
                        child,
                        focus_position,
                    )
                }
            },
        }
    }
}

#[derive(PartialEq, Eq, Debug, Clone)]
pub struct GlobalCursor {
    pub value_cursor: ValueCursor,
    pub line_cursor: Option<LineCursor>,
}
impl GlobalCursor {
    pub fn new(jsons: Rc<[JV]>, width: u16) -> Option<Self> {
        let cursor = ValueCursor::new(jsons)?;
        let line_cursor = match &cursor.focus {
            // Note that this is only valid because a top-level string renders across the entire
            // rect, so rect width is equal to the line width
            &JV::String(ref s) => Some(LineCursor::new_at_start(s.clone(), width)),
            _ => None,
        };
        Some(GlobalCursor {
            value_cursor: cursor,
            line_cursor,
        })
    }
    pub fn new_end(jsons: Rc<[JV]>, width: u16) -> Option<Self> {
        let cursor = ValueCursor::new_end(jsons)?;
        let line_cursor = match &cursor.focus {
            // Note that this is only valid because a top-level string renders across the entire
            // rect, so rect width is equal to the line width
            &JV::String(ref s) => Some(LineCursor::new_at_end(s.clone(), width)),
            _ => None,
        };
        Some(GlobalCursor {
            value_cursor: cursor,
            line_cursor,
        })
    }
    pub fn current_line(&self, folds: &HashSet<(usize, Vec<usize>)>) -> Line {
        self.value_cursor
            .current_line(folds, self.line_cursor.as_ref())
    }
    pub fn render_lines(
        &mut self,
        cursor: Option<&ValueCursor>,
        folds: &HashSet<(usize, Vec<usize>)>,
        rect: Rect,
    ) -> Vec<Spans<'static>> {
        let mut lines = Vec::with_capacity(rect.height as usize);
        if let Some(c) = self.line_cursor.as_mut() {
            c.set_width(rect.width);
        }
        lines.push(
            self.current_line(folds)
                .render(Some(&self.value_cursor) == cursor, rect.width),
        );
        while lines.len() < rect.height as usize {
            if let None = self.advance(folds, rect) {
                break;
            };
            lines.push(
                self.current_line(folds)
                    .render(Some(&self.value_cursor) == cursor, rect.width),
            );
        }
        lines
    }
    pub fn advance(&mut self, folds: &HashSet<(usize, Vec<usize>)>, rect: Rect) -> Option<()> {
        if let Some(lc) = self.line_cursor.as_mut() {
            lc.move_next();
            if lc.current().is_some() {
                return Some(());
            }
        }
        self.value_cursor.advance(folds)?;
        if let JV::String(ref value) = &self.value_cursor.focus {
            let width = self.value_cursor.content_width(rect.width);
            self.line_cursor = Some(LineCursor::new_at_start(value.clone(), width));
        }
        Some(())
    }
    pub fn regress(&mut self, folds: &HashSet<(usize, Vec<usize>)>, rect: Rect) -> Option<()> {
        if let Some(lc) = self.line_cursor.as_mut() {
            lc.move_prev();
            if lc.current().is_some() {
                return Some(());
            }
        }
        self.value_cursor.regress(folds)?;
        if let JV::String(ref value) = &self.value_cursor.focus {
            let width = self.value_cursor.content_width(rect.width);
            self.line_cursor = Some(LineCursor::new_at_end(value.clone(), width));
        }
        Some(())
    }
}

#[derive(PartialEq, Eq, Debug, Clone)]
pub struct ValueCursor {
    // Top level jsons of the view
    pub jsons: Rc<[JV]>,
    // Index locating the json this cursor is focused (somewhere) on
    pub top_index: usize,
    // Stores the ancestors of the current focus, the index of their focused child, and an iterator
    // that will continue right after that child.
    pub frames: Vec<CursorFrame>,
    // Currently focused json value
    pub focus: JV,
    // If the json is an array or object, indicates whether the currently focused line is the
    // opening or closing bracket.
    pub focus_position: FocusPosition,
}

impl ValueCursor {
    pub fn new(jsons: Rc<[JV]>) -> Option<Self> {
        let focus = jsons.first()?.clone();
        let focus_position = FocusPosition::starting(&focus);
        Some(ValueCursor {
            jsons,
            top_index: 0,
            frames: Vec::new(),
            focus,
            focus_position,
        })
    }
    pub fn new_end(jsons: Rc<[JV]>) -> Option<Self> {
        let top_index = jsons.len() - 1;
        let focus = jsons.last()?.clone();
        let focus_position = FocusPosition::ending(&focus);
        Some(ValueCursor {
            jsons,
            top_index,
            frames: Vec::new(),
            focus,
            focus_position,
        })
    }
    pub fn to_path(&self) -> Path {
        Path {
            top_index: self.top_index,
            frames: self.frames.iter().map(CursorFrame::index).collect(),
            focus_position: self.focus_position,
        }
    }
    pub fn from_path(jsons: Rc<[JV]>, path: &Path) -> Self {
        let mut focus = jsons[path.top_index].clone();
        let mut frames = Vec::new();
        for &index in path.frames.iter() {
            match focus {
                JV::Array(arr) => {
                    let json = arr.clone();
                    focus = arr
                        .get(index as i32)
                        .expect("Shape of path does not match shape of jsons");
                    frames.push(CursorFrame::Array { index, json });
                }
                JV::Object(obj) => {
                    let json = obj.clone();
                    let mut iterator = obj.clone().into_iter();
                    let (key, new_focus) = iterator
                        .nth(index)
                        .expect("Shape of path does not match shape of jsons");
                    focus = new_focus;
                    frames.push(CursorFrame::Object {
                        index,
                        json,
                        key,
                        iterator,
                    });
                }
                _ => panic!("Shape of path does not match shape of jsons"),
            }
        }
        ValueCursor {
            jsons,
            top_index: path.top_index,
            frames,
            focus,
            focus_position: path.focus_position,
        }
    }
    pub fn current_key(&self) -> Option<&str> {
        match self.focus_position {
            FocusPosition::End => None,
            _ => match self.frames.last() {
                None => None,
                Some(CursorFrame::Array { .. }) => None,
                Some(CursorFrame::Object { key, .. }) => Some(key),
            },
        }
    }
    pub fn current_indent(&self) -> u16 {
        (self.frames.len() * 2) as u16
    }
    pub fn content_width(&self, width: u16) -> u16 {
        let key = self.current_key();
        let indent = self.current_indent();
        let indent_span = Span::raw(" ".repeat(indent as usize));
        let out = match key {
            Some(key) => vec![
                indent_span,
                Span::raw(format!("\"{}\"", escaped_str(key))),
                Span::raw(" : "),
            ],
            _ => vec![indent_span],
        };
        let consumed_width: u16 = out.iter().map(|span| span.width() as u16).sum();
        let remainining_width = width.saturating_sub(consumed_width);
        remainining_width
    }
    pub fn current_line<'a>(
        &'a self,
        folds: &HashSet<(usize, Vec<usize>)>,
        line_cursor: Option<&'a LineCursor>,
    ) -> Line<'a> {
        use FocusPosition::*;
        let folded = folds.contains(&self.to_path().strip_position());
        let content = match (&self.focus, self.focus_position, folded) {
            (JV::Object(_), Start, false) => LineContent::ObjectStart,
            (JV::Object(_), End, false) => LineContent::ObjectEnd,
            (JV::Object(obj), Start, true) => LineContent::FoldedObject(obj.len() as usize),
            (JV::Array(_), Start, false) => LineContent::ArrayStart,
            (JV::Array(_), End, false) => LineContent::ArrayEnd,
            (JV::Array(arr), Start, true) => LineContent::FoldedArray(arr.len() as usize),
            (JV::Null(_), Value, _) => LineContent::Null,
            (JV::Bool(b), Value, _) => LineContent::Bool(b.value()),
            (JV::Number(x), Value, _) => LineContent::Number(x.value()),
            (JV::String(_), Value, _) => {
                // TODO: this is bad for many reasons. Ideally in the future every type will use
                // LineCursor (since keys could be very long, for example). But at the very least,
                // we should make it so LineCursor and ValueCursor have the same semantics about
                // moving: either they should both refuse to move off the edge, or both should
                // allow it.
                LineContent::String(line_cursor.unwrap().current().unwrap())
            }
            triple => panic!("Illegal json/focus_position/folded triple: {:?}", triple),
        };
        let key = self.current_key();
        let comma = match self.focus_position {
            FocusPosition::Start => false,
            _ => match self.frames.last() {
                None => false,
                Some(CursorFrame::Array { json, index, .. }) => *index != json.len() as usize - 1,
                Some(CursorFrame::Object { iterator, .. }) => iterator.len() != 0,
            },
        };
        let indent = self.current_indent();
        Line {
            content,
            key,
            comma,
            indent,
        }
    }
    pub fn advance(&mut self, folds: &HashSet<(usize, Vec<usize>)>) -> Option<()> {
        // This gets pretty deep into nested match statements, so an english guide to what's going
        // on here.
        // Cases:
        // * We're focused on an open bracket. Push a new frame and start in on the contents of the
        // container. (open_container)
        // * We're focused on a leaf...
        //   * and we have no parent, so advance the very top level, or roll off the end.
        //   * and we have a parent... (Frame::advance)
        //     * and there are more leaves, so focus on the next leaf.
        //     * and there are no more leaves, so pop the frame, focus on the parent's close bracket
        // * We're focused on a close bracket. Advance the parent as if we were focused on a leaf.
        let is_folded = folds.contains(&self.to_path().strip_position());
        match self.focus_position {
            FocusPosition::Start if !is_folded => {
                let (new_frame, new_focus, new_focus_position) = open_container(self.focus.clone());
                if let Some(new_frame) = new_frame {
                    self.frames.push(new_frame);
                }
                self.focus = new_focus;
                self.focus_position = new_focus_position;
            }
            _ => match self.frames.pop() {
                None => {
                    self.focus = self.jsons.get(self.top_index + 1)?.clone();
                    self.top_index += 1;
                    self.focus_position = FocusPosition::starting(&self.focus);
                }
                Some(frame) => {
                    let (new_frame, new_focus, new_focus_position) = frame.advance();
                    if let Some(new_frame) = new_frame {
                        self.frames.push(new_frame);
                    }
                    self.focus = new_focus;
                    self.focus_position = new_focus_position;
                }
            },
        }
        Some(())
    }
    pub fn regress(&mut self, folds: &HashSet<(usize, Vec<usize>)>) -> Option<()> {
        // Pretty mechanical opposite of advance
        match self.focus_position {
            FocusPosition::End => {
                let (new_frame, new_focus, new_focus_position) =
                    open_container_end(self.focus.clone());
                if let Some(new_frame) = new_frame {
                    self.frames.push(new_frame);
                }
                self.focus = new_focus;
                self.focus_position = new_focus_position;
            }
            FocusPosition::Value | FocusPosition::Start => match self.frames.pop() {
                None => {
                    self.top_index = self.top_index.checked_sub(1)?;
                    self.focus = self.jsons[self.top_index].clone();
                    self.focus_position = FocusPosition::ending(&self.focus);
                }
                Some(frame) => {
                    let (new_frame, new_focus, new_focus_position) = frame.regress();
                    if let Some(new_frame) = new_frame {
                        self.frames.push(new_frame);
                    }
                    self.focus = new_focus;
                    self.focus_position = new_focus_position;
                }
            },
        }
        let is_folded = folds.contains(&self.to_path().strip_position());
        if is_folded {
            self.focus_position = FocusPosition::Start;
        }
        Some(())
    }
    fn leaf_to_string(&self) -> Option<Cow<str>> {
        match &self.focus {
            JV::Null(_) => Some("null".into()),
            JV::Bool(b) => Some(b.value().to_string().into()),
            JV::Number(x) => Some(x.value().to_string().into()),
            JV::String(s) => Some(s.value().into()),
            _ => None,
        }
    }
    // TODO: do something more efficient
    pub fn matches_path(&self, path: &Path) -> bool {
        self.to_path() == *path
    }
    pub fn regex_matches(&self, re: &Regex) -> bool {
        if let Some(leaf) = self.leaf_to_string() {
            if re.is_match(&leaf) {
                return true;
            }
        }
        if let Some(CursorFrame::Object { key, .. }) = self.frames.last() {
            if re.is_match(key) {
                return true;
            }
        }
        false
    }
    pub fn search(mut self, re: &Regex) -> Option<Self> {
        let mock_folds = HashSet::new();
        let start = self.to_path();
        while let Some(()) = self.advance(&mock_folds) {
            if self.regex_matches(re) {
                return Some(self);
            }
        }
        let mut cursor = ValueCursor::new(self.jsons).expect("Jsons can't be empty here");
        while !cursor.matches_path(&start) {
            if cursor.regex_matches(re) {
                return Some(cursor);
            }
            cursor
                .advance(&mock_folds)
                .expect("Shouldn't hit end again before hitting initial position");
        }
        None
    }
    pub fn search_back(mut self, re: &Regex) -> Option<Self> {
        let mock_folds = HashSet::new();
        let start = self.to_path();
        while let Some(()) = self.regress(&mock_folds) {
            if self.regex_matches(re) {
                return Some(self);
            }
        }
        let mut cursor = ValueCursor::new_end(self.jsons).expect("Jsons can't be empty here");
        while !cursor.matches_path(&start) {
            if cursor.regex_matches(re) {
                return Some(cursor);
            }
            cursor
                .regress(&mock_folds)
                .expect("Shouldn't hit start again before hitting initial position");
        }
        None
    }
    pub fn descends_from_or_matches(&self, other: &Self) -> bool {
        if self.top_index != other.top_index {
            return false;
        }
        if self.frames.len() < other.frames.len() {
            return false;
        }
        self.frames
            .iter()
            .zip(other.frames.iter())
            .all(|(self_frame, other_frame)| self_frame == other_frame)
    }
}

#[derive(PartialEq, Eq, Hash, Debug, Clone)]
pub struct Path {
    top_index: usize,
    frames: Vec<usize>,
    focus_position: FocusPosition,
}
impl Path {
    pub fn strip_position(self) -> (usize, Vec<usize>) {
        let Path {
            top_index,
            frames,
            focus_position: _,
        } = self;
        (top_index, frames)
    }
}

impl PartialOrd for Path {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Path {
    fn cmp(&self, other: &Self) -> Ordering {
        match self.top_index.cmp(&other.top_index) {
            Ordering::Equal => {}
            ordering => return ordering,
        }
        let mut self_frames = self.frames.iter();
        let mut other_frames = other.frames.iter();
        loop {
            match (self_frames.next(), other_frames.next()) {
                (Some(self_frame), Some(other_frame)) => match self_frame.cmp(other_frame) {
                    Ordering::Equal => {}
                    ordering => return ordering,
                },
                (None, Some(_)) => match self.focus_position {
                    FocusPosition::Start => return Ordering::Less,
                    FocusPosition::Value => {
                        panic!("Cannot compare paths that index different jsons")
                    }
                    FocusPosition::End => return Ordering::Greater,
                },
                (Some(_), None) => match other.focus_position {
                    FocusPosition::Start => return Ordering::Greater,
                    FocusPosition::Value => {
                        panic!("Cannot compare paths that index different jsons")
                    }
                    FocusPosition::End => return Ordering::Less,
                },
                (None, None) => return self.focus_position.cmp(&other.focus_position),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ValueCursor;
    use crate::{
        jq::jv::JV,
        testing::{arb_json, json_to_lines},
    };
    use pretty_assertions::assert_eq;
    use proptest::proptest;
    use serde_json::{json, Value};
    use std::{collections::HashSet, rc::Rc};

    fn check_advancing_terminates(jsons: Vec<Value>) {
        let jsons: Vec<JV> = jsons.iter().map(|v| v.into()).collect();
        let folds = HashSet::new();
        if let Some(mut cursor) = ValueCursor::new(jsons.into()) {
            let mut last_path = cursor.to_path();
            while let Some(()) = cursor.advance(&folds) {
                let path = cursor.to_path();
                assert_ne!(last_path, path);
                last_path = path;
            }
        }
    }
    #[test]
    fn unit_advancing_terminates() {
        check_advancing_terminates(vec![json![{}]]);
    }
    proptest! {
        #[test]
        fn prop_lines(values in proptest::collection::vec(arb_json(), 1..10)) {
            let jsons : Vec<JV> = values.iter().map(|v| v.into()).collect();
            let folds = HashSet::new();
            let mut expected_lines = json_to_lines(values.iter()).into_iter();
            if let Some(mut cursor) = ValueCursor::new(jsons.into()) {
                let mut actual_lines = Vec::new();
                actual_lines.push(cursor.current_line(&folds));
                assert_eq!(cursor.current_line(&folds), expected_lines.next().expect("Expected lines shorter than actual lines"));
                while let Some(()) = cursor.advance(&folds) {
                    assert_eq!(cursor.current_line(&folds), expected_lines.next().expect("Expected lines shorter than actual lines"));
                }
            }
            assert!(expected_lines.next().is_none());
        }
    }
    fn check_path_roundtrip(cursor: &ValueCursor, jsons: Rc<[JV]>) {
        let path = cursor.to_path();
        let new_cursor = ValueCursor::from_path(jsons, &path);
        assert_eq!(*cursor, new_cursor);
    }
    proptest! {
        #[test]
        fn prop_path_roundtrip(values in proptest::collection::vec(arb_json(), 1..10)) {
            let jsons : Vec<JV> = values.iter().map(|v| v.into()).collect();
            let jsons : Rc<[JV]> = jsons.into();
            let folds = HashSet::new();
            if let Some(mut cursor) = ValueCursor::new(jsons.clone()) {
                check_path_roundtrip(&cursor, jsons.clone());
                while let Some(()) = cursor.advance(&folds) {
                    check_path_roundtrip(&cursor, jsons.clone());
                }
            }
        }
    }
    fn check_advance_regress(cursor: &ValueCursor, folds: &HashSet<(usize, Vec<usize>)>) {
        let mut actual: ValueCursor = cursor.clone();
        if actual.advance(folds).is_none() {
            return;
        }
        actual.regress(folds).unwrap();
        assert_eq!(actual, *cursor);
    }
    proptest! {
        #[test]
        fn prop_advance_regress(values in proptest::collection::vec(arb_json(), 1..10)) {
            let jsons : Vec<JV> = values.iter().map(|v| v.into()).collect();
            let jsons : Rc<[JV]> = jsons.into();
            let folds = HashSet::new();
            if let Some(mut cursor) = ValueCursor::new(jsons.clone()) {
                check_advance_regress(&cursor, &folds);
                while let Some(()) = cursor.advance(&folds) {
                    check_advance_regress(&cursor, &folds);
                }
            }
        }
    }
    proptest! {
        #[test]
        fn prop_path_ordering(values in proptest::collection::vec(arb_json(), 1..10)) {
            let jsons : Vec<JV> = values.iter().map(|v| v.into()).collect();
            let jsons : Rc<[JV]> = jsons.into();
            let folds = HashSet::new();
            if let Some(mut cursor) = ValueCursor::new(jsons) {
                let mut prior_path = cursor.to_path();
                while let Some(()) = cursor.advance(&folds) {
                    let new_path = cursor.to_path();
                    dbg!(&new_path, &prior_path);
                    assert!(new_path > prior_path, "Expected {:?} > {:?}", &new_path, &prior_path);
                    prior_path = new_path;
                }
            }
        }
    }
}
