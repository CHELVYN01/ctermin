//! The split pane tree: each leaf is one pane, each branch divides its space in two.

use eframe::egui::{pos2, vec2, Pos2, Rect};

use super::PaneId;

const DEFAULT_RATIO: f32 = 0.5;
const MIN_PANE_SIZE: f32 = 48.0;
const EPSILON: f32 = 0.001;

/// The thickness of the divider between two panes.
pub const DIVIDER_THICKNESS: f32 = 2.0;

/// The direction space is divided in.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Axis {
    /// Panes side by side, left and right.
    Horizontal,
    /// Panes stacked, top and bottom.
    Vertical,
}

/// The direction focus moves in.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
}

/// A branch on the path from the tree root to a split.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Branch {
    First,
    Second,
}

#[derive(Debug, PartialEq)]
pub enum Layout {
    Leaf(PaneId),
    Split {
        axis: Axis,
        ratio: f32,
        a: Box<Layout>,
        b: Box<Layout>,
    },
}

/// A draggable divider and the split it controls.
#[derive(Debug)]
pub struct Divider {
    pub path: Vec<Branch>,
    pub axis: Axis,
    pub rect: Rect,
    parent: Rect,
}

impl Divider {
    /// The split ratio that puts the divider at the pointer, without making a pane too small.
    pub fn ratio_at(&self, pointer: Pos2) -> f32 {
        let (start, extent, position) = match self.axis {
            Axis::Horizontal => (self.parent.left(), self.parent.width(), pointer.x),
            Axis::Vertical => (self.parent.top(), self.parent.height(), pointer.y),
        };
        let available = extent - DIVIDER_THICKNESS;
        if available <= 0.0 {
            return DEFAULT_RATIO;
        }
        let minimum = (MIN_PANE_SIZE / available).min(DEFAULT_RATIO);
        ((position - start) / available).clamp(minimum, 1.0 - minimum)
    }
}

impl Layout {
    /// Arranges panes in a balanced grid, alternating direction at each level.
    pub fn balanced(panes: &[PaneId], axis: Axis) -> Option<Self> {
        match panes {
            [] => None,
            [pane] => Some(Self::Leaf(*pane)),
            _ => {
                let middle = panes.len().div_ceil(2);
                let (first, second) = panes.split_at(middle);
                let next = match axis {
                    Axis::Horizontal => Axis::Vertical,
                    Axis::Vertical => Axis::Horizontal,
                };
                Some(Self::Split {
                    axis,
                    ratio: middle as f32 / panes.len() as f32,
                    a: Box::new(Self::balanced(first, next)?),
                    b: Box::new(Self::balanced(second, next)?),
                })
            }
        }
    }

    /// Splits pane `target`. The new pane goes right or below.
    pub fn split(&mut self, target: PaneId, new: PaneId, axis: Axis) -> bool {
        match self {
            Self::Leaf(pane) if *pane == target => {
                *self = Self::Split {
                    axis,
                    ratio: DEFAULT_RATIO,
                    a: Box::new(Self::Leaf(target)),
                    b: Box::new(Self::Leaf(new)),
                };
                true
            }
            Self::Leaf(_) => false,
            Self::Split { a, b, .. } => a.split(target, new, axis) || b.split(target, new, axis),
        }
    }

    /// Removes a pane; its sibling takes the parent's space. `None` if the tree becomes empty.
    pub fn remove(self, target: PaneId) -> Option<Self> {
        match self {
            Self::Leaf(pane) if pane == target => None,
            Self::Leaf(pane) => Some(Self::Leaf(pane)),
            Self::Split { axis, ratio, a, b } => match (a.remove(target), b.remove(target)) {
                (Some(a), Some(b)) => Some(Self::Split {
                    axis,
                    ratio,
                    a: Box::new(a),
                    b: Box::new(b),
                }),
                (Some(remaining), None) | (None, Some(remaining)) => Some(remaining),
                (None, None) => None,
            },
        }
    }

    /// The pane that should take focus after `target` closes.
    pub fn sibling_of(&self, target: PaneId) -> Option<PaneId> {
        let Self::Split { a, b, .. } = self else {
            return None;
        };
        if **a == Self::Leaf(target) {
            return Some(b.first_pane());
        }
        if **b == Self::Leaf(target) {
            return Some(a.first_pane());
        }
        a.sibling_of(target).or_else(|| b.sibling_of(target))
    }

    pub fn first_pane(&self) -> PaneId {
        match self {
            Self::Leaf(pane) => *pane,
            Self::Split { a, .. } => a.first_pane(),
        }
    }

    pub fn panes(&self) -> Vec<PaneId> {
        self.arrange(unit_rect(), 0.0)
            .into_iter()
            .map(|(pane, _)| pane)
            .collect()
    }

    /// The screen rect of each pane inside `rect`.
    pub fn arrange(&self, rect: Rect, gap: f32) -> Vec<(PaneId, Rect)> {
        let mut result = Vec::new();
        self.collect_rects(rect, gap, &mut result);
        result
    }

    fn collect_rects(&self, rect: Rect, gap: f32, result: &mut Vec<(PaneId, Rect)>) {
        match self {
            Self::Leaf(pane) => result.push((*pane, rect)),
            Self::Split { axis, ratio, a, b } => {
                let (first, _, second) = split_rect(rect, *axis, *ratio, gap);
                a.collect_rects(first, gap, result);
                b.collect_rects(second, gap, result);
            }
        }
    }

    pub fn dividers(&self, rect: Rect) -> Vec<Divider> {
        let mut result = Vec::new();
        self.collect_dividers(rect, &mut Vec::new(), &mut result);
        result
    }

    fn collect_dividers(&self, rect: Rect, path: &mut Vec<Branch>, result: &mut Vec<Divider>) {
        let Self::Split { axis, ratio, a, b } = self else {
            return;
        };
        let (first, divider, second) = split_rect(rect, *axis, *ratio, DIVIDER_THICKNESS);
        result.push(Divider {
            path: path.clone(),
            axis: *axis,
            rect: divider,
            parent: rect,
        });
        path.push(Branch::First);
        a.collect_dividers(first, path, result);
        path.pop();
        path.push(Branch::Second);
        b.collect_dividers(second, path, result);
        path.pop();
    }

    pub fn set_ratio(&mut self, path: &[Branch], new_ratio: f32) {
        let Self::Split { ratio, a, b, .. } = self else {
            return;
        };
        match path.split_first() {
            None => *ratio = new_ratio,
            Some((Branch::First, rest)) => a.set_ratio(rest, new_ratio),
            Some((Branch::Second, rest)) => b.set_ratio(rest, new_ratio),
        }
    }

    /// The closest pane from `from` in a direction. If there are several, the one sharing the
    /// longest edge is chosen.
    pub fn neighbor(&self, from: PaneId, direction: Direction) -> Option<PaneId> {
        let rects = self.arrange(unit_rect(), 0.0);
        let (_, current) = rects.iter().find(|(pane, _)| *pane == from)?;
        rects
            .iter()
            .filter(|(pane, _)| *pane != from)
            .filter_map(|(pane, rect)| {
                let (distance, overlap) = reach(*current, *rect, direction);
                (distance > -EPSILON && overlap > EPSILON).then_some((*pane, distance, overlap))
            })
            .min_by(|left, right| left.1.total_cmp(&right.1).then(right.2.total_cmp(&left.2)))
            .map(|(pane, _, _)| pane)
    }
}

/// A reference space for calculations that depend only on proportions.
fn unit_rect() -> Rect {
    Rect::from_min_size(Pos2::ZERO, vec2(1000.0, 1000.0))
}

/// The distance to another rect in a direction, and the length of their shared edge.
fn reach(from: Rect, to: Rect, direction: Direction) -> (f32, f32) {
    let vertical = from.top().max(to.top())..from.bottom().min(to.bottom());
    let horizontal = from.left().max(to.left())..from.right().min(to.right());
    match direction {
        Direction::Left => (from.left() - to.right(), vertical.end - vertical.start),
        Direction::Right => (to.left() - from.right(), vertical.end - vertical.start),
        Direction::Up => (from.top() - to.bottom(), horizontal.end - horizontal.start),
        Direction::Down => (to.top() - from.bottom(), horizontal.end - horizontal.start),
    }
}

/// Splits a rect into the first part, the divider, and the second part.
fn split_rect(rect: Rect, axis: Axis, ratio: f32, gap: f32) -> (Rect, Rect, Rect) {
    match axis {
        Axis::Horizontal => {
            let edge = (rect.left() + (rect.width() - gap) * ratio).round();
            (
                Rect::from_min_max(rect.min, pos2(edge, rect.bottom())),
                Rect::from_min_max(pos2(edge, rect.top()), pos2(edge + gap, rect.bottom())),
                Rect::from_min_max(pos2(edge + gap, rect.top()), rect.max),
            )
        }
        Axis::Vertical => {
            let edge = (rect.top() + (rect.height() - gap) * ratio).round();
            (
                Rect::from_min_max(rect.min, pos2(rect.right(), edge)),
                Rect::from_min_max(pos2(rect.left(), edge), pos2(rect.right(), edge + gap)),
                Rect::from_min_max(pos2(rect.left(), edge + gap), rect.max),
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::Tabs;

    fn ids<const N: usize>() -> [PaneId; N] {
        let mut tabs = Tabs::default();
        std::array::from_fn(|_| tabs.open())
    }

    fn screen() -> Rect {
        Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0))
    }

    /// Four panes: A on the left, B top right, then C and D bottom right.
    ///
    /// ```text
    /// +---+-------+
    /// |   |   B   |
    /// | A +---+---+
    /// |   | C | D |
    /// +---+---+---+
    /// ```
    fn four_panes() -> (Layout, [PaneId; 4]) {
        let [a, b, c, d] = ids();
        let mut layout = Layout::Leaf(a);
        layout.split(a, b, Axis::Horizontal);
        layout.split(b, c, Axis::Vertical);
        layout.split(c, d, Axis::Horizontal);
        (layout, [a, b, c, d])
    }

    #[test]
    fn balanced_grid_shares_space_evenly() {
        let panes: [PaneId; 4] = ids();
        let layout = Layout::balanced(&panes, Axis::Horizontal).expect("has a pane");
        let rects = layout.arrange(screen(), 0.0);

        assert_eq!(rects.len(), 4);
        for (_, rect) in &rects {
            assert!((rect.area() - screen().area() / 4.0).abs() < 1000.0);
        }
    }

    #[test]
    fn odd_grid_stays_proportional() {
        let panes: [PaneId; 3] = ids();
        let layout = Layout::balanced(&panes, Axis::Horizontal).expect("has a pane");
        let rects = layout.arrange(screen(), 0.0);
        assert_eq!(rects.len(), 3);
        assert!(rects[0].1.width() > rects[2].1.width());
        assert!(Layout::balanced(&[], Axis::Horizontal).is_none());
    }

    #[test]
    fn split_divides_in_two_with_the_new_pane_on_the_right() {
        let [a, b] = ids();
        let mut layout = Layout::Leaf(a);
        assert!(layout.split(a, b, Axis::Horizontal));

        let rects = layout.arrange(screen(), DIVIDER_THICKNESS);
        assert_eq!(rects[0].0, a);
        assert_eq!(rects[1].0, b);
        assert!(rects[0].1.right() < rects[1].1.left());
        assert_eq!(rects[0].1.height(), 600.0);
    }

    #[test]
    fn top_bottom_split() {
        let [a, b] = ids();
        let mut layout = Layout::Leaf(a);
        layout.split(a, b, Axis::Vertical);
        let rects = layout.arrange(screen(), DIVIDER_THICKNESS);
        assert!(rects[0].1.bottom() < rects[1].1.top());
        assert_eq!(rects[0].1.width(), 800.0);
    }

    #[test]
    fn split_of_missing_target_keeps_the_layout() {
        let [a, b, c] = ids();
        let mut layout = Layout::Leaf(a);
        assert!(!layout.split(b, c, Axis::Horizontal));
        assert_eq!(layout, Layout::Leaf(a));
    }

    #[test]
    fn nested_splits_fill_the_space_without_overlap() {
        let (layout, panes) = four_panes();
        let rects = layout.arrange(screen(), DIVIDER_THICKNESS);
        assert_eq!(rects.len(), panes.len());

        let dividers = layout.dividers(screen());
        assert_eq!(dividers.len(), 3);

        let pane_area: f32 = rects.iter().map(|(_, rect)| rect.area()).sum();
        let divider_area: f32 = dividers.iter().map(|divider| divider.rect.area()).sum();
        assert!((pane_area + divider_area - screen().area()).abs() < 0.01);

        for (index, (_, first)) in rects.iter().enumerate() {
            for (_, second) in &rects[index + 1..] {
                let overlap = first.intersect(*second);
                assert!(!overlap.is_positive(), "{first:?} overlaps {second:?}");
            }
        }
    }

    #[test]
    fn closing_a_pane_gives_its_space_to_the_sibling() {
        let [a, b, c] = ids();
        let mut layout = Layout::Leaf(a);
        layout.split(a, b, Axis::Horizontal);
        layout.split(b, c, Axis::Vertical);

        let layout = layout.remove(c).expect("a pane remains");
        let rects = layout.arrange(screen(), DIVIDER_THICKNESS);
        assert_eq!(rects.len(), 2);
        assert_eq!(rects[1].0, b);
        assert_eq!(rects[1].1.height(), 600.0, "b takes the full height");
    }

    #[test]
    fn closing_a_middle_pane_keeps_the_rest_intact() {
        let (layout, [a, b, c, d]) = four_panes();
        let layout = layout.remove(b).expect("a pane remains");
        assert_eq!(layout.panes(), vec![a, c, d]);

        let rects = layout.arrange(screen(), DIVIDER_THICKNESS);
        let area: f32 = rects.iter().map(|(_, rect)| rect.area()).sum();
        let dividers: f32 = layout
            .dividers(screen())
            .iter()
            .map(|divider| divider.rect.area())
            .sum();
        assert!((area + dividers - screen().area()).abs() < 0.01);
    }

    #[test]
    fn closing_the_last_pane_empties_the_tree() {
        let [a] = ids();
        assert_eq!(Layout::Leaf(a).remove(a), None);
    }

    #[test]
    fn sibling_pane_for_the_next_focus() {
        let (layout, [a, b, c, d]) = four_panes();
        assert_eq!(layout.sibling_of(c), Some(d));
        assert_eq!(layout.sibling_of(d), Some(c));
        assert_eq!(layout.sibling_of(b), Some(c));
        assert_eq!(layout.sibling_of(a), Some(b));
        assert_eq!(Layout::Leaf(a).sibling_of(a), None);
    }

    #[test]
    fn neighbors_follow_screen_position() {
        let (layout, [a, b, c, d]) = four_panes();
        assert_eq!(layout.neighbor(a, Direction::Right), Some(b));
        assert_eq!(layout.neighbor(b, Direction::Left), Some(a));
        assert_eq!(layout.neighbor(c, Direction::Left), Some(a));
        assert_eq!(layout.neighbor(c, Direction::Right), Some(d));
        assert_eq!(layout.neighbor(d, Direction::Up), Some(b));
        assert_eq!(layout.neighbor(b, Direction::Down), Some(c));
    }

    #[test]
    fn no_neighbor_at_the_edges() {
        let (layout, [a, b, _, d]) = four_panes();
        assert_eq!(layout.neighbor(a, Direction::Left), None);
        assert_eq!(layout.neighbor(a, Direction::Up), None);
        assert_eq!(layout.neighbor(b, Direction::Up), None);
        assert_eq!(layout.neighbor(d, Direction::Right), None);
        assert_eq!(layout.neighbor(d, Direction::Down), None);
    }

    #[test]
    fn ratio_follows_the_pointer_within_pane_limits() {
        let [a, b] = ids();
        let mut layout = Layout::Leaf(a);
        layout.split(a, b, Axis::Horizontal);
        let dividers = layout.dividers(screen());
        let divider = &dividers[0];

        let middle = divider.ratio_at(pos2(399.0, 0.0));
        assert!((middle - 0.5).abs() < 0.01);

        let minimum = MIN_PANE_SIZE / (800.0 - DIVIDER_THICKNESS);
        assert!((divider.ratio_at(pos2(-50.0, 0.0)) - minimum).abs() < 0.001);
        assert!((divider.ratio_at(pos2(900.0, 0.0)) - (1.0 - minimum)).abs() < 0.001);
    }

    #[test]
    fn ratio_is_safe_in_tiny_spaces() {
        let [a, b] = ids();
        let mut layout = Layout::Leaf(a);
        layout.split(a, b, Axis::Vertical);
        let tiny = Rect::from_min_size(Pos2::ZERO, vec2(100.0, 1.0));
        let dividers = layout.dividers(tiny);
        assert_eq!(dividers[0].ratio_at(pos2(0.0, 0.0)), DEFAULT_RATIO);

        let small = Rect::from_min_size(Pos2::ZERO, vec2(100.0, 50.0));
        let dividers = layout.dividers(small);
        assert_eq!(dividers[0].ratio_at(pos2(0.0, 10.0)), DEFAULT_RATIO);
    }

    #[test]
    fn path_changes_the_right_split_ratio() {
        let (mut layout, [_, b, c, d]) = four_panes();
        let dividers = layout.dividers(screen());
        let nested = dividers
            .iter()
            .find(|divider| divider.path == [Branch::Second, Branch::Second])
            .expect("the C and D split must have a divider");
        let path = nested.path.clone();

        layout.set_ratio(&path, 0.25);

        let rects = layout.arrange(screen(), DIVIDER_THICKNESS);
        let rect = |pane| rects.iter().find(|(id, _)| *id == pane).map(|(_, r)| *r);
        let (b, c, d) = (rect(b), rect(c), rect(d));
        assert!(c.expect("c exists").width() < d.expect("d exists").width() / 2.0);
        assert!(
            b.expect("b exists").width() > 300.0,
            "other splits are unchanged"
        );
    }
}
