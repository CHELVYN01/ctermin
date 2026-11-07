//! The tab list and active tab. Each tab has its own pane tree and focused pane.

use super::tree::{Axis, Direction, Layout};

/// The unique identity of a pane. Never reused while the app runs.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct PaneId(u64);

struct Tab {
    layout: Layout,
    focused: PaneId,
}

#[derive(Default)]
pub struct Tabs {
    tabs: Vec<Tab>,
    active: usize,
    next_id: u64,
}

impl Tabs {
    fn new_pane(&mut self) -> PaneId {
        let pane = PaneId(self.next_id);
        self.next_id += 1;
        pane
    }

    /// Opens a new tab at the far right and activates it.
    pub fn open(&mut self) -> PaneId {
        self.open_after(self.tabs.len())
    }

    /// Opens one tab with `count` panes in a balanced grid.
    pub fn open_group(&mut self, count: usize) -> Vec<PaneId> {
        let panes: Vec<PaneId> = (0..count).map(|_| self.new_pane()).collect();
        if let (Some(layout), Some(first)) = (
            Layout::balanced(&panes, Axis::Horizontal),
            panes.first().copied(),
        ) {
            self.tabs.push(Tab {
                layout,
                focused: first,
            });
            self.active = self.tabs.len() - 1;
        }
        panes
    }

    /// Opens a new tab right after `index` and activates it.
    pub fn open_after(&mut self, index: usize) -> PaneId {
        let pane = self.new_pane();
        let position = (index + 1).min(self.tabs.len());
        self.tabs.insert(
            position,
            Tab {
                layout: Layout::Leaf(pane),
                focused: pane,
            },
        );
        self.active = position;
        pane
    }

    /// Closes a tab and all its panes. The active tab stays the same, unless that tab is the one
    /// closed; then the tab at the same position (or the last tab) becomes active.
    pub fn close(&mut self, index: usize) -> Vec<PaneId> {
        if index >= self.tabs.len() {
            return Vec::new();
        }
        let tab = self.tabs.remove(index);
        if index < self.active {
            self.active -= 1;
        }
        self.active = self.active.min(self.tabs.len().saturating_sub(1));
        tab.layout.panes()
    }

    /// Splits the focused pane. The new pane takes focus.
    pub fn split_focused(&mut self, axis: Axis) -> Option<PaneId> {
        self.tabs.get(self.active)?;
        let new = self.new_pane();
        let tab = self.tabs.get_mut(self.active)?;
        tab.layout.split(tab.focused, new, axis);
        tab.focused = new;
        Some(new)
    }

    /// Closes the focused pane. The last pane closes its tab.
    pub fn close_focused(&mut self) -> Option<PaneId> {
        let tab = self.tabs.get_mut(self.active)?;
        let closed = tab.focused;
        let Some(sibling) = tab.layout.sibling_of(closed) else {
            self.close(self.active);
            return Some(closed);
        };
        let layout = std::mem::replace(&mut tab.layout, Layout::Leaf(sibling));
        if let Some(remaining) = layout.remove(closed) {
            tab.layout = remaining;
        }
        tab.focused = sibling;
        Some(closed)
    }

    pub fn focus(&mut self, pane: PaneId) {
        if let Some(tab) = self.tabs.get_mut(self.active) {
            if tab.layout.panes().contains(&pane) {
                tab.focused = pane;
            }
        }
    }

    pub fn focus_direction(&mut self, direction: Direction) {
        if let Some(tab) = self.tabs.get_mut(self.active) {
            if let Some(pane) = tab.layout.neighbor(tab.focused, direction) {
                tab.focused = pane;
            }
        }
    }

    pub fn select(&mut self, index: usize) {
        if index < self.tabs.len() {
            self.active = index;
        }
    }

    pub fn next(&mut self) {
        if !self.tabs.is_empty() {
            self.active = (self.active + 1) % self.tabs.len();
        }
    }

    pub fn previous(&mut self) {
        if !self.tabs.is_empty() {
            self.active = (self.active + self.tabs.len() - 1) % self.tabs.len();
        }
    }

    /// The focused pane of the active tab.
    pub fn active(&self) -> Option<PaneId> {
        self.tabs.get(self.active).map(|tab| tab.focused)
    }

    pub fn active_index(&self) -> usize {
        self.active
    }

    pub fn active_layout(&self) -> Option<&Layout> {
        self.tabs.get(self.active).map(|tab| &tab.layout)
    }

    pub fn active_layout_mut(&mut self) -> Option<&mut Layout> {
        self.tabs.get_mut(self.active).map(|tab| &mut tab.layout)
    }

    /// The focused pane of each tab, from the left. The tab title follows this pane.
    pub fn focused_panes(&self) -> Vec<PaneId> {
        self.tabs.iter().map(|tab| tab.focused).collect()
    }

    /// The pane count of each tab, from the left.
    pub fn pane_counts(&self) -> Vec<usize> {
        self.tabs
            .iter()
            .map(|tab| tab.layout.panes().len())
            .collect()
    }

    pub fn is_empty(&self) -> bool {
        self.tabs.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tabs(count: usize) -> Tabs {
        let mut tabs = Tabs::default();
        for _ in 0..count {
            tabs.open();
        }
        tabs
    }

    #[test]
    fn new_tab_becomes_active_with_a_unique_id() {
        let mut tabs = tabs(2);
        let pane = tabs.open();
        assert_eq!(tabs.active(), Some(pane));
        assert_eq!(tabs.active_index(), 2);

        tabs.close(2);
        assert_ne!(tabs.open(), pane, "ids must not be reused");
    }

    #[test]
    fn duplicate_tab_sits_right_of_its_source() {
        let mut tabs = tabs(3);
        let last = tabs.focused_panes()[2];
        let new = tabs.open_after(0);

        assert_eq!(tabs.focused_panes()[1], new);
        assert_eq!(tabs.focused_panes()[3], last);
        assert_eq!(tabs.active_index(), 1);
    }

    #[test]
    fn pane_group_becomes_one_active_tab() {
        let mut tabs = tabs(1);
        let panes = tabs.open_group(3);

        assert_eq!(tabs.pane_counts(), vec![1, 3]);
        assert_eq!(tabs.active_index(), 1);
        assert_eq!(tabs.active(), panes.first().copied());
        assert!(tabs.open_group(0).is_empty());
        assert_eq!(tabs.pane_counts().len(), 2, "an empty group creates no tab");
    }

    #[test]
    fn pane_count_per_tab() {
        let mut tabs = tabs(2);
        tabs.split_focused(Axis::Horizontal);
        assert_eq!(tabs.pane_counts(), vec![1, 2]);
    }

    #[test]
    fn closing_the_active_tab_selects_the_same_position() {
        let mut tabs = tabs(3);
        let last = tabs.focused_panes()[2];
        tabs.select(1);
        tabs.close(1);
        assert_eq!(tabs.active(), Some(last));
    }

    #[test]
    fn closing_the_rightmost_active_tab_selects_the_previous() {
        let mut tabs = tabs(3);
        let middle = tabs.focused_panes()[1];
        tabs.close(2);
        assert_eq!(tabs.active(), Some(middle));
    }

    #[test]
    fn closing_another_tab_keeps_the_active_tab() {
        let mut tabs = tabs(3);
        let active = tabs.focused_panes()[2];
        tabs.close(0);
        assert_eq!(tabs.active(), Some(active));

        let mut tabs = self::tabs(3);
        tabs.select(0);
        let active = tabs.active();
        tabs.close(2);
        assert_eq!(tabs.active(), active);
    }

    #[test]
    fn closing_the_last_tab_empties_the_list() {
        let mut tabs = tabs(1);
        assert_eq!(tabs.close(0).len(), 1);
        assert!(tabs.is_empty());
        assert_eq!(tabs.active(), None);
        assert!(tabs.close(0).is_empty());
    }

    #[test]
    fn tab_switching_wraps() {
        let mut tabs = tabs(3);
        tabs.next();
        assert_eq!(tabs.active_index(), 0);
        tabs.previous();
        assert_eq!(tabs.active_index(), 2);
    }

    #[test]
    fn operations_without_tabs_do_not_panic() {
        let mut tabs = Tabs::default();
        tabs.next();
        tabs.previous();
        tabs.select(3);
        tabs.focus_direction(Direction::Left);
        assert_eq!(tabs.split_focused(Axis::Horizontal), None);
        assert_eq!(tabs.close_focused(), None);
        assert_eq!(tabs.active(), None);
    }

    #[test]
    fn split_moves_focus_to_the_new_pane() {
        let mut tabs = tabs(1);
        let first = tabs.active();
        let new = tabs.split_focused(Axis::Horizontal);
        assert_eq!(tabs.active(), new);
        assert_ne!(tabs.active(), first);
        assert_eq!(tabs.active_layout().map(|l| l.panes().len()), Some(2));
    }

    #[test]
    fn closing_the_focused_pane_moves_focus_to_its_sibling() {
        let mut tabs = tabs(1);
        let first = tabs.active().expect("has a tab");
        let second = tabs
            .split_focused(Axis::Horizontal)
            .expect("split succeeded");

        assert_eq!(tabs.close_focused(), Some(second));
        assert_eq!(tabs.active(), Some(first));
        assert!(!tabs.is_empty(), "the tab remains");
    }

    #[test]
    fn closing_the_last_pane_closes_its_tab() {
        let mut tabs = tabs(2);
        tabs.close_focused();
        assert_eq!(tabs.focused_panes().len(), 1);
    }

    #[test]
    fn closing_a_tab_returns_all_panes() {
        let mut tabs = tabs(1);
        tabs.split_focused(Axis::Horizontal);
        tabs.split_focused(Axis::Vertical);
        assert_eq!(tabs.close(0).len(), 3);
    }

    #[test]
    fn each_tab_keeps_its_own_focus() {
        let mut tabs = tabs(2);
        let second_tab = tabs.split_focused(Axis::Horizontal);
        tabs.select(0);
        tabs.select(1);
        assert_eq!(tabs.active(), second_tab);
    }

    #[test]
    fn focus_follows_direction_and_click() {
        let mut tabs = tabs(1);
        let left = tabs.active().expect("has a tab");
        let right = tabs
            .split_focused(Axis::Horizontal)
            .expect("split succeeded");

        tabs.focus_direction(Direction::Left);
        assert_eq!(tabs.active(), Some(left));
        tabs.focus_direction(Direction::Left);
        assert_eq!(tabs.active(), Some(left), "no movement at the edge");

        tabs.focus(right);
        assert_eq!(tabs.active(), Some(right));
    }

    #[test]
    fn focus_ignores_panes_from_other_tabs() {
        let mut tabs = tabs(2);
        let other_tab = tabs.focused_panes()[0];
        let active = tabs.active();
        tabs.focus(other_tab);
        assert_eq!(tabs.active(), active);
    }
}
