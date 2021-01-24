use tui::layout::{Constraint, Direction, Layout, Rect};
#[derive(Clone, Copy, Debug)]
pub struct JexLayout {
    pub tree: Option<Rect>,
    pub left: Rect,
    pub right: Rect,
    pub query: Rect,
}

impl JexLayout {
    pub fn new(size: Rect, show_tree: bool) -> JexLayout {
        let vchunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(0), Constraint::Length(1)].as_ref())
            .split(size);
        if show_tree {
            let tree_split = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Length(20), Constraint::Ratio(1, 1)].as_ref())
                .split(vchunks[0]);
            let views = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Ratio(1, 2), Constraint::Ratio(1, 2)].as_ref())
                .split(tree_split[1]);
            JexLayout {
                tree: Some(tree_split[0]),
                left: views[0],
                right: views[1],
                query: vchunks[1],
            }
        } else {
            let views = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Ratio(1, 2), Constraint::Ratio(1, 2)].as_ref())
                .split(vchunks[0]);
            JexLayout {
                tree: None,
                left: views[0],
                right: views[1],
                query: vchunks[1],
            }
        }
    }
}

pub fn flash(size: Rect) -> Rect {
    let v_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Ratio(1, 4),
            Constraint::Ratio(1, 2),
            Constraint::Ratio(1, 4),
        ])
        .split(size);
    let h_layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Ratio(1, 4),
            Constraint::Ratio(1, 2),
            Constraint::Ratio(1, 4),
        ])
        .split(v_layout[1]);
    h_layout[1]
}
