use crate::{
    jq::jv::{JVArray, JVObject, JVString, OwnedObjectIterator, JV},
    lines::{Leaf, LeafContent, LineCursor, UnstyledSpans},
};
use log::trace;
use regex::Regex;
use std::{borrow::Cow, cmp::Ordering, collections::HashSet, fmt, rc::Rc};
use tui::{layout::Rect, text::Spans};

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
        key: JVString,
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

#[derive(Debug, Clone)]
pub struct GlobalCursor {
    pub value_cursor: LeafCursor,
    pub line_cursor: LineCursor,
}
impl GlobalCursor {
    pub fn new(jsons: Rc<[JV]>, width: u16, folds: &HashSet<(usize, Vec<usize>)>) -> Option<Self> {
        let cursor = LeafCursor::new(jsons)?;
        let line = cursor.current_line(folds, width);
        let line_cursor = LineCursor::new_at_start(line.render(), width);
        Some(GlobalCursor {
            value_cursor: cursor,
            line_cursor,
        })
    }
    pub fn new_end(
        jsons: Rc<[JV]>,
        width: u16,
        folds: &HashSet<(usize, Vec<usize>)>,
    ) -> Option<Self> {
        let cursor = LeafCursor::new_end(jsons)?;
        let line = cursor.current_line(folds, width);
        let line_cursor = LineCursor::new_at_start(line.render(), width);
        Some(GlobalCursor {
            value_cursor: cursor,
            line_cursor,
        })
    }
    pub fn current_line(&self) -> UnstyledSpans {
        self.line_cursor
            .current()
            .expect("Global cursor should not be able to have invalid line cursor")
    }
    pub fn render_lines(
        &mut self,
        cursor: Option<&LeafCursor>,
        folds: &HashSet<(usize, Vec<usize>)>,
        rect: Rect,
    ) -> Vec<Spans<'static>> {
        let mut lines = Vec::with_capacity(rect.height as usize);
        self.resize_to(rect);
        lines.push(
            self.current_line()
                .to_spans(Some(&self.value_cursor) == cursor),
        );
        while lines.len() < rect.height as usize {
            if let None = self.advance(folds, rect.width) {
                break;
            };
            lines.push(
                self.current_line()
                    .to_spans(Some(&self.value_cursor) == cursor),
            );
        }
        lines
    }
    pub fn advance(&mut self, folds: &HashSet<(usize, Vec<usize>)>, width: u16) -> Option<()> {
        trace!("Advancing global cursor (width={}): {:#?}", width, self);
        let lc = &mut self.line_cursor;
        lc.move_next();
        if lc.valid() {
            trace!("Advanced global cursor {:#?}", self);
            return Some(());
        } else {
            lc.move_prev();
        }
        self.value_cursor.advance(folds)?;
        let line = self.value_cursor.current_line(folds, width);
        self.line_cursor = LineCursor::new_at_start(line.render(), width);
        trace!("Advanced global cursor {:#?}", self);
        Some(())
    }
    pub fn regress(&mut self, folds: &HashSet<(usize, Vec<usize>)>, width: u16) -> Option<()> {
        let lc = &mut self.line_cursor;
        lc.move_prev();
        if lc.valid() {
            return Some(());
        } else {
            lc.move_next();
        }
        self.value_cursor.regress(folds)?;
        let line = self.value_cursor.current_line(folds, width);
        self.line_cursor = LineCursor::new_at_end(line.render(), width);
        Some(())
    }
    pub fn to_path(&self) -> GlobalPath {
        let current_line = self
            .line_cursor
            .current_line()
            .expect("GlobalCursor should not have invalid LineCursor");
        GlobalPath {
            value_path: self.value_cursor.to_path(),
            current_line,
        }
    }
    pub fn resize_to(&mut self, rect: Rect) {
        self.line_cursor.set_width(rect.width);
    }
    pub fn at_line_end(&self) -> bool {
        self.line_cursor
            .at_end()
            .expect("GlobalCursor should not contain invalid LineCursor")
    }
}

#[derive(PartialEq, Eq, Debug, Clone)]
pub struct LeafCursor {
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

impl LeafCursor {
    pub fn new(jsons: Rc<[JV]>) -> Option<Self> {
        let focus = jsons.first()?.clone();
        let focus_position = FocusPosition::starting(&focus);
        Some(LeafCursor {
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
        Some(LeafCursor {
            jsons,
            top_index,
            frames: Vec::new(),
            focus,
            focus_position,
        })
    }
    pub fn to_path(&self) -> ValuePath {
        ValuePath {
            top_index: self.top_index,
            frames: self.frames.iter().map(CursorFrame::index).collect(),
            focus_position: self.focus_position,
        }
    }
    pub fn from_path(jsons: Rc<[JV]>, path: &ValuePath) -> Self {
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
        LeafCursor {
            jsons,
            top_index: path.top_index,
            frames,
            focus,
            focus_position: path.focus_position,
        }
    }
    pub fn current_key(&self) -> Option<JVString> {
        match self.focus_position {
            FocusPosition::End => None,
            _ => match self.frames.last() {
                None => None,
                Some(CursorFrame::Array { .. }) => None,
                Some(CursorFrame::Object { key, .. }) => Some(key.clone()),
            },
        }
    }
    pub fn current_indent(&self, width: u16) -> u16 {
        let desired_indent = (self.frames.len() * 2) as u16;
        std::cmp::min(desired_indent, width - 7)
    }
    pub fn current_line<'a>(&'a self, folds: &HashSet<(usize, Vec<usize>)>, width: u16) -> Leaf {
        use FocusPosition::*;
        let folded = folds.contains(&self.to_path().strip_position());
        let content = match (&self.focus, self.focus_position, folded) {
            (JV::Object(_), Start, false) => LeafContent::ObjectStart,
            (JV::Object(_), End, false) => LeafContent::ObjectEnd,
            (JV::Object(obj), Start, true) => LeafContent::FoldedObject(obj.len() as usize),
            (JV::Array(_), Start, false) => LeafContent::ArrayStart,
            (JV::Array(_), End, false) => LeafContent::ArrayEnd,
            (JV::Array(arr), Start, true) => LeafContent::FoldedArray(arr.len() as usize),
            (JV::Null(_), Value, _) => LeafContent::Null,
            (JV::Bool(b), Value, _) => LeafContent::Bool(b.value()),
            (JV::Number(x), Value, _) => LeafContent::Number(x.value()),
            (JV::String(s), Value, _) => LeafContent::String(s.clone()),
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
        let indent = self.current_indent(width);
        Leaf {
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
    pub fn matches_path(&self, path: &ValuePath) -> bool {
        self.to_path() == *path
    }
    pub fn regex_matches(&self, re: &Regex) -> bool {
        if let Some(leaf) = self.leaf_to_string() {
            if re.is_match(&leaf) {
                return true;
            }
        }
        if let Some(CursorFrame::Object { key, .. }) = self.frames.last() {
            if re.is_match(key.value()) {
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
        let mut cursor = LeafCursor::new(self.jsons).expect("Jsons can't be empty here");
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
        let mut cursor = LeafCursor::new_end(self.jsons).expect("Jsons can't be empty here");
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
pub struct ValuePath {
    top_index: usize,
    frames: Vec<usize>,
    focus_position: FocusPosition,
}
impl ValuePath {
    pub fn strip_position(self) -> (usize, Vec<usize>) {
        let ValuePath {
            top_index,
            frames,
            focus_position: _,
        } = self;
        (top_index, frames)
    }
}

impl PartialOrd for ValuePath {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for ValuePath {
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

#[derive(PartialEq, Eq, Debug, Clone, PartialOrd, Ord)]
pub struct GlobalPath {
    pub value_path: ValuePath,
    pub current_line: usize,
}

#[cfg(test)]
mod tests {
    use super::{GlobalCursor, LeafCursor};
    use crate::{
        jq::jv::JV,
        lines::LineCursor,
        testing::{arb_json, json_to_lines},
    };
    use pretty_assertions::assert_eq;
    use proptest::proptest;
    use serde_json::{json, Value};
    use std::{collections::HashSet, rc::Rc};

    fn check_advancing_terminates(jsons: Vec<Value>) {
        let jsons: Vec<JV> = jsons.iter().map(|v| v.into()).collect();
        let folds = HashSet::new();
        if let Some(mut cursor) = LeafCursor::new(jsons.into()) {
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
    fn check_lines(values: Vec<Value>) {
        let jsons: Vec<JV> = values.iter().map(|v| v.into()).collect();
        let folds = HashSet::new();
        let width = u16::MAX;
        let mut expected_lines = json_to_lines(values.iter()).into_iter();
        if let Some(mut cursor) = GlobalCursor::new(jsons.into(), width, &folds) {
            let mut actual_lines = Vec::new();
            actual_lines.push(cursor.current_line());
            let expected_line = expected_lines
                .next()
                .expect("Expected lines shorter than actual lines");
            let expected = LineCursor::new_at_start(expected_line.render(), width)
                .current()
                .unwrap();
            assert_eq!(cursor.current_line(), expected);
            while let Some(()) = cursor.advance(&folds, width) {
                let expected_line = expected_lines
                    .next()
                    .expect("Expected lines shorter than actual lines");
                let expected = LineCursor::new_at_start(expected_line.render(), width)
                    .current()
                    .unwrap();
                assert_eq!(cursor.current_line(), expected);
            }
        }
        assert!(expected_lines.next().is_none());
    }
    proptest! {
        #[test]
        fn prop_lines(values in proptest::collection::vec(arb_json(), 1..10)) {
            check_lines(values);
        }
    }
    #[test]
    fn unit_lines() {
        check_lines(vec![json!([{ "": null }])]);
    }
    fn check_path_roundtrip_inner(cursor: &LeafCursor, jsons: Rc<[JV]>) {
        let path = cursor.to_path();
        let new_cursor = LeafCursor::from_path(jsons, &path);
        assert_eq!(*cursor, new_cursor);
    }
    fn check_path_roundtrip(values: Vec<serde_json::Value>) {
        let jsons: Vec<JV> = values.iter().map(|v| v.into()).collect();
        let jsons: Rc<[JV]> = jsons.into();
        let folds = HashSet::new();
        if let Some(mut cursor) = LeafCursor::new(jsons.clone()) {
            check_path_roundtrip_inner(&cursor, jsons.clone());
            while let Some(()) = cursor.advance(&folds) {
                check_path_roundtrip_inner(&cursor, jsons.clone());
            }
        }
    }
    #[test]
    fn unit_path_roundtrip() {
        check_path_roundtrip(vec![json!([{ "": null }])])
    }
    proptest! {
        #[test]
        fn prop_path_roundtrip(values in proptest::collection::vec(arb_json(), 1..10)) {
            check_path_roundtrip(values)
        }
    }
    fn check_advance_regress(
        cursor: &GlobalCursor,
        folds: &HashSet<(usize, Vec<usize>)>,
        width: u16,
    ) {
        let mut actual = cursor.clone();
        if actual.advance(folds, width).is_none() {
            return;
        }
        actual.regress(folds, width).unwrap();
        assert_eq!(actual.to_path(), cursor.to_path());
    }
    fn hashable_cursor_key(cursor: &GlobalCursor) -> impl std::hash::Hash + Eq {
        (
            cursor.value_cursor.to_path(),
            cursor.line_cursor.current_line(),
        )
    }
    proptest! {
        fn prop_advance_regress(values in proptest::collection::vec(arb_json(), 1..10), width in 8u16..250) {
            let jsons : Vec<JV> = values.iter().map(|v| v.into()).collect();
            let jsons : Rc<[JV]> = jsons.into();
            let folds = HashSet::new();
            let mut seen = HashSet::new();
            if let Some(mut cursor) = GlobalCursor::new(jsons.clone(), width, &folds) {
                check_advance_regress(&cursor, &folds, width);
                while let Some(()) = cursor.advance(&folds, width) {
                    let key = hashable_cursor_key(&cursor);
                    if seen.contains(&key) {
                        panic!("Infinite loop");
                    }
                    seen.insert(key);
                    check_advance_regress(&cursor, &folds, width);
                }
            }
        }
    }
    #[test]
    fn unit_advance_regress() {
        let tests = vec![
            (vec![json!([""])], 50),
            (vec![json!("aaa\u{e000}¡")], 8),
            (vec![json!([[{"\u{20f1}¡¡a": "\u{b}"}]])], 16),
            (vec![json!([[{"\u{20f1}¡¡a": "\u{b}"}]])], 16),
            (
                vec![json!([[{"¡¡": "\u{0}\u{0}\u{7f}\u{3fffe}®\u{e000}A0\u{3fffe}𠀀\""}]])],
                8,
            ),
        ];
        for (values, width) in tests {
            let jsons: Vec<JV> = values.iter().map(|v| v.into()).collect();
            let jsons: Rc<[JV]> = jsons.into();
            let folds = HashSet::new();
            let mut seen = HashSet::new();
            if let Some(mut cursor) = GlobalCursor::new(jsons.clone(), width, &folds) {
                check_advance_regress(&cursor, &folds, width);
                while let Some(()) = cursor.advance(&folds, width) {
                    let key = hashable_cursor_key(&cursor);
                    if seen.contains(&key) {
                        panic!("Infinite loop");
                    }
                    seen.insert(key);
                    check_advance_regress(&cursor, &folds, width);
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
            if let Some(mut cursor) = LeafCursor::new(jsons) {
                let mut prior_path = cursor.to_path();
                while let Some(()) = cursor.advance(&folds) {
                    let new_path = cursor.to_path();
                    assert!(new_path > prior_path, "Expected {:?} > {:?}", &new_path, &prior_path);
                    prior_path = new_path;
                }
            }
        }
    }
}
