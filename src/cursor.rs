use crate::{
    jq::jv::{JVArray, JVObject, JV},
    lines::{Line, LineContent},
};
use std::collections::HashSet;

// Requirements:
// * Produce the current line
// * Step forward
// * (Optionally, for searching): Step backwards
// * Can be "dehydrated" into something hashable for storing folds (other metadata?)

#[derive(PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Clone, Copy)]
enum FocusPosition {
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

pub enum CursorFrame {
    Array {
        index: usize,
        json: JVArray,
    },
    Object {
        index: usize,
        key: String,
        json: JVObject,
        iterator: Box<dyn ExactSizeIterator<Item = (String, JV)>>,
    },
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
            let mut iterator = Box::new(obj.clone().into_iter());
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
                            json: obj,
                            key,
                            iterator: Box::new(std::iter::empty()),
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
                    let focus_position = FocusPosition::starting(&child);
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
                    let mut iterator = Box::new(json.clone().into_iter());
                    let (key, child) = iterator
                        .nth(index)
                        .expect("Stepped back and didn't find a child");
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
}

#[derive(PartialEq, Eq)]
pub struct Cursor<'a> {
    // Top level jsons of the view
    jsons: &'a [JV],
    // Index locating the json this cursor is focused (somewhere) on
    top_index: usize,
    // Stores the ancestors of the current focus, the index of their focused child, and an iterator
    // that will continue right after that child.
    frames: Vec<CursorFrame>,
    // Currently focused json value
    focus: JV,
    // If the json is an array or object, indicates whether the currently focused line is the
    // opening or closing bracket.
    focus_position: FocusPosition,
}

impl<'a> Cursor<'a> {
    pub fn new(jsons: &'a [JV]) -> Option<Self> {
        let focus = jsons.get(0)?.clone();
        let focus_position = FocusPosition::starting(&focus);
        Some(Cursor {
            jsons,
            top_index: 0,
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
    pub fn from_path(jsons: &'a [JV], path: &Path) -> Self {
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
                    let mut iterator = Box::new(obj.clone().into_iter());
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
        Cursor {
            jsons,
            top_index: path.top_index,
            frames,
            focus,
            focus_position: path.focus_position,
        }
    }
    pub fn current_line(&self, folds: &HashSet<Path>) -> Line {
        use FocusPosition::*;
        let content = match (&self.focus, self.focus_position) {
            (JV::Object(_), Start) => LineContent::ObjectStart(0),
            (JV::Object(_), End) => LineContent::ObjectEnd(0),
            (JV::Array(_), Start) => LineContent::ArrayStart(0),
            (JV::Array(_), End) => LineContent::ArrayEnd(0),
            (JV::Null(_), Value) => LineContent::Null,
            (JV::Bool(b), Value) => LineContent::Bool(b.value()),
            (JV::Number(x), Value) => LineContent::Number(x.value()),
            (JV::String(s), Value) => LineContent::String(s.value().clone().into()),
            pair => panic!("Illegal json/focus_position pair: {:?}", pair),
        };
        let key = match self.focus_position {
            FocusPosition::End => None,
            _ => match self.frames.last() {
                None => None,
                Some(CursorFrame::Array { .. }) => None,
                Some(CursorFrame::Object { key, .. }) => Some(key.clone().into()),
            },
        };
        let folded = folds.contains(&self.to_path());
        let comma = match self.focus_position {
            FocusPosition::Start => false,
            _ => match self.frames.last() {
                None => false,
                Some(CursorFrame::Array { json, index, .. }) => *index != json.len() as usize - 1,
                Some(CursorFrame::Object { iterator, .. }) => iterator.len() != 0,
            },
        };
        let indent = self.frames.len() as u8;
        Line {
            content,
            key,
            folded,
            comma,
            indent,
        }
    }
    pub fn advance(&mut self) -> Option<()> {
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
        match self.focus_position {
            FocusPosition::Start => {
                let (new_frame, new_focus, new_focus_position) = open_container(self.focus.clone());
                if let Some(new_frame) = new_frame {
                    self.frames.push(new_frame);
                }
                self.focus = new_focus;
                self.focus_position = new_focus_position;
            }
            FocusPosition::Value | FocusPosition::End => match self.frames.pop() {
                None => {
                    self.top_index += 1;
                    self.focus = self.jsons.get(self.top_index)?.clone();
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
    pub fn regress(&mut self) -> Option<()> {
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
        Some(())
    }
}

#[derive(PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Clone)]
pub struct Path {
    top_index: usize,
    frames: Vec<usize>,
    focus_position: FocusPosition,
}

#[cfg(test)]
mod tests {
    use super::Cursor;
    use crate::{
        jq::jv::JV,
        lines::{Line, LineContent},
        testing::{arb_json, json_to_lines},
    };
    use pretty_assertions::assert_eq;
    use proptest::proptest;
    use std::collections::HashSet;

    fn strip_container_sizes(lines: &mut [Line]) {
        for line in lines {
            match &mut line.content {
                LineContent::ArrayStart(x)
                | LineContent::ArrayEnd(x)
                | LineContent::ObjectStart(x)
                | LineContent::ObjectEnd(x) => *x = 0,
                _ => {}
            }
        }
    }

    proptest! {
        #[test]
        fn prop_lines(values in proptest::collection::vec(arb_json(), 1..10)) {
            let jsons : Vec<JV> = values.iter().map(|v| v.into()).collect();
            let folds = HashSet::new();
            let mut actual_lines = Vec::new();
            if let Some(mut cursor) = Cursor::new(&jsons) {
                actual_lines.push(cursor.current_line(&folds));
                while let Some(()) = cursor.advance() {
                    actual_lines.push(cursor.current_line(&folds));
                }
            }
            let mut expected_lines = json_to_lines(values.iter());
            strip_container_sizes(&mut expected_lines);
            assert_eq!(actual_lines, expected_lines);
        }
    }
    fn check_path_roundtrip(cursor: &Cursor, jsons: &[JV]) {
        let path = cursor.to_path();
        let new_cursor = Cursor::from_path(jsons, &path);
        assert!(*cursor == new_cursor);
    }
    proptest! {
        #[test]
        fn prop_path_roundtrip(values in proptest::collection::vec(arb_json(), 1..10)) {
            let jsons : Vec<JV> = values.iter().map(|v| v.into()).collect();
            if let Some(mut cursor) = Cursor::new(&jsons) {
                check_path_roundtrip(&cursor, &jsons);
                while let Some(()) = cursor.advance() {
                    check_path_roundtrip(&cursor, &jsons);
                }
            }
        }
    }
}
