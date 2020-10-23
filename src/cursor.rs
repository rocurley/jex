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
}

pub enum CursorFrame {
    Array {
        index: usize,
        json: JVArray,
        iterator: Box<dyn ExactSizeIterator<Item = JV>>,
    },
    Object {
        index: usize,
        key: String,
        json: JVObject,
        iterator: Box<dyn ExactSizeIterator<Item = (String, JV)>>,
    },
}

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
                            iterator,
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

impl CursorFrame {
    pub fn index(&self) -> usize {
        match self {
            CursorFrame::Array { index, .. } => *index,
            CursorFrame::Object { index, .. } => *index,
        }
    }
    fn advance(self) -> (Option<Self>, JV, FocusPosition) {
        use CursorFrame::*;
        match self {
            Array {
                index,
                json,
                mut iterator,
            } => match iterator.next() {
                None => (None, json.into(), FocusPosition::End),
                Some(child) => {
                    let focus_position = FocusPosition::starting(&child);
                    (
                        Some(Array {
                            index: index + 1,
                            json,
                            iterator,
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
}

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
    pub fn to_path(&self) -> Path {
        Path {
            top_index: self.top_index,
            frames: self.frames.iter().map(CursorFrame::index).collect(),
            focus_position: self.focus_position,
        }
    }
    pub fn current_line(&self, folds: HashSet<Path>) -> Line {
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
        let key = match self.frames.last() {
            None => None,
            Some(CursorFrame::Array { .. }) => None,
            Some(CursorFrame::Object { key, .. }) => Some(key.clone().into()),
        };
        let folded = folds.contains(&self.to_path());
        let comma = match self.frames.last() {
            None => false,
            Some(CursorFrame::Array { iterator, .. }) => iterator.len() != 0,
            Some(CursorFrame::Object { iterator, .. }) => iterator.len() != 0,
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
    pub fn advance(&mut self) {
        // Cases:
        // * We're focused on an open bracket. Push a new frame and start in on the contents of the
        // container.
        // * We're focused on a leaf...
        //   * and there are more leaves, so focus on the next leaf.
        //   * and there are no more leaves...
        //     * and we have a parent, so pop the frame, focus on the parent's close bracket
        //     * and we have no parent, so advance the very top level, or roll off the end.
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
                    // TODO: this will fail at the end of the arrays
                    self.focus = self.jsons[self.top_index].clone();
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
    }
}

#[derive(PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Clone)]
pub struct Path {
    top_index: usize,
    frames: Vec<usize>,
    focus_position: FocusPosition,
}
