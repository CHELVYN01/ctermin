//! Daftar tab dan tab aktif. Tiap tab punya tree panel dan panel fokusnya sendiri.

use super::tree::{Axis, Direction, Layout};

/// Identitas unik satu panel. Tidak pernah dipakai ulang selama aplikasi berjalan.
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

    /// Membuka tab baru di ujung kanan dan langsung mengaktifkannya.
    pub fn open(&mut self) -> PaneId {
        self.open_after(self.tabs.len())
    }

    /// Membuka satu tab berisi `count` panel dalam grid seimbang.
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

    /// Membuka tab baru tepat di sebelah kanan `index` dan langsung mengaktifkannya.
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

    /// Menutup tab beserta semua panelnya. Tab aktif tetap sama, kecuali tab itu sendiri yang
    /// ditutup; dalam hal itu tab di posisi yang sama (atau tab terakhir) menjadi aktif.
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

    /// Membagi panel fokus. Panel baru langsung menerima fokus.
    pub fn split_focused(&mut self, axis: Axis) -> Option<PaneId> {
        self.tabs.get(self.active)?;
        let new = self.new_pane();
        let tab = self.tabs.get_mut(self.active)?;
        tab.layout.split(tab.focused, new, axis);
        tab.focused = new;
        Some(new)
    }

    /// Menutup panel fokus. Panel terakhir menutup tabnya.
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

    /// Panel fokus pada tab aktif.
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

    /// Panel fokus tiap tab, urut dari kiri. Judul tab mengikuti panel ini.
    pub fn focused_panes(&self) -> Vec<PaneId> {
        self.tabs.iter().map(|tab| tab.focused).collect()
    }

    /// Jumlah panel tiap tab, urut dari kiri.
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
    fn tab_baru_menjadi_aktif_dengan_id_unik() {
        let mut tabs = tabs(2);
        let pane = tabs.open();
        assert_eq!(tabs.active(), Some(pane));
        assert_eq!(tabs.active_index(), 2);

        tabs.close(2);
        assert_ne!(tabs.open(), pane, "id tidak boleh dipakai ulang");
    }

    #[test]
    fn tab_duplikat_berada_tepat_di_kanan_sumbernya() {
        let mut tabs = tabs(3);
        let last = tabs.focused_panes()[2];
        let new = tabs.open_after(0);

        assert_eq!(tabs.focused_panes()[1], new);
        assert_eq!(tabs.focused_panes()[3], last);
        assert_eq!(tabs.active_index(), 1);
    }

    #[test]
    fn grup_panel_menjadi_satu_tab_aktif() {
        let mut tabs = tabs(1);
        let panes = tabs.open_group(3);

        assert_eq!(tabs.pane_counts(), vec![1, 3]);
        assert_eq!(tabs.active_index(), 1);
        assert_eq!(tabs.active(), panes.first().copied());
        assert!(tabs.open_group(0).is_empty());
        assert_eq!(tabs.pane_counts().len(), 2, "grup kosong tidak membuat tab");
    }

    #[test]
    fn jumlah_panel_tiap_tab() {
        let mut tabs = tabs(2);
        tabs.split_focused(Axis::Horizontal);
        assert_eq!(tabs.pane_counts(), vec![1, 2]);
    }

    #[test]
    fn menutup_tab_aktif_memilih_tab_di_posisi_sama() {
        let mut tabs = tabs(3);
        let last = tabs.focused_panes()[2];
        tabs.select(1);
        tabs.close(1);
        assert_eq!(tabs.active(), Some(last));
    }

    #[test]
    fn menutup_tab_aktif_paling_kanan_memilih_tab_sebelumnya() {
        let mut tabs = tabs(3);
        let middle = tabs.focused_panes()[1];
        tabs.close(2);
        assert_eq!(tabs.active(), Some(middle));
    }

    #[test]
    fn menutup_tab_lain_tidak_mengubah_tab_aktif() {
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
    fn menutup_tab_terakhir_mengosongkan_daftar() {
        let mut tabs = tabs(1);
        assert_eq!(tabs.close(0).len(), 1);
        assert!(tabs.is_empty());
        assert_eq!(tabs.active(), None);
        assert!(tabs.close(0).is_empty());
    }

    #[test]
    fn pindah_tab_berputar() {
        let mut tabs = tabs(3);
        tabs.next();
        assert_eq!(tabs.active_index(), 0);
        tabs.previous();
        assert_eq!(tabs.active_index(), 2);
    }

    #[test]
    fn operasi_tanpa_tab_tidak_panik() {
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
    fn split_memindahkan_fokus_ke_panel_baru() {
        let mut tabs = tabs(1);
        let first = tabs.active();
        let new = tabs.split_focused(Axis::Horizontal);
        assert_eq!(tabs.active(), new);
        assert_ne!(tabs.active(), first);
        assert_eq!(tabs.active_layout().map(|l| l.panes().len()), Some(2));
    }

    #[test]
    fn menutup_panel_fokus_memindahkan_fokus_ke_saudara() {
        let mut tabs = tabs(1);
        let first = tabs.active().expect("ada tab");
        let second = tabs
            .split_focused(Axis::Horizontal)
            .expect("split berhasil");

        assert_eq!(tabs.close_focused(), Some(second));
        assert_eq!(tabs.active(), Some(first));
        assert!(!tabs.is_empty(), "tab tetap ada");
    }

    #[test]
    fn menutup_panel_terakhir_menutup_tabnya() {
        let mut tabs = tabs(2);
        tabs.close_focused();
        assert_eq!(tabs.focused_panes().len(), 1);
    }

    #[test]
    fn menutup_tab_mengembalikan_semua_panel() {
        let mut tabs = tabs(1);
        tabs.split_focused(Axis::Horizontal);
        tabs.split_focused(Axis::Vertical);
        assert_eq!(tabs.close(0).len(), 3);
    }

    #[test]
    fn fokus_tiap_tab_disimpan_sendiri() {
        let mut tabs = tabs(2);
        let second_tab = tabs.split_focused(Axis::Horizontal);
        tabs.select(0);
        tabs.select(1);
        assert_eq!(tabs.active(), second_tab);
    }

    #[test]
    fn fokus_mengikuti_arah_dan_klik() {
        let mut tabs = tabs(1);
        let left = tabs.active().expect("ada tab");
        let right = tabs
            .split_focused(Axis::Horizontal)
            .expect("split berhasil");

        tabs.focus_direction(Direction::Left);
        assert_eq!(tabs.active(), Some(left));
        tabs.focus_direction(Direction::Left);
        assert_eq!(tabs.active(), Some(left), "tidak bergerak di tepi");

        tabs.focus(right);
        assert_eq!(tabs.active(), Some(right));
    }

    #[test]
    fn fokus_mengabaikan_panel_dari_tab_lain() {
        let mut tabs = tabs(2);
        let other_tab = tabs.focused_panes()[0];
        let active = tabs.active();
        tabs.focus(other_tab);
        assert_eq!(tabs.active(), active);
    }
}
