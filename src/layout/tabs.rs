//! Daftar tab dan tab aktif, tanpa ketergantungan ke UI maupun sesi terminal.

/// Identitas unik satu panel. Tidak pernah dipakai ulang selama aplikasi berjalan.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct PaneId(u64);

#[derive(Default)]
pub struct Tabs {
    panes: Vec<PaneId>,
    active: usize,
    next_id: u64,
}

impl Tabs {
    /// Membuka tab baru di ujung kanan dan langsung mengaktifkannya.
    pub fn open(&mut self) -> PaneId {
        let pane = PaneId(self.next_id);
        self.next_id += 1;
        self.panes.push(pane);
        self.active = self.panes.len() - 1;
        pane
    }

    /// Menutup tab. Tab aktif tetap sama, kecuali tab itu sendiri yang ditutup; dalam hal itu
    /// tab di posisi yang sama (atau tab terakhir) menjadi aktif.
    pub fn close(&mut self, index: usize) -> Option<PaneId> {
        if index >= self.panes.len() {
            return None;
        }
        let pane = self.panes.remove(index);
        if index < self.active {
            self.active -= 1;
        }
        self.active = self.active.min(self.panes.len().saturating_sub(1));
        Some(pane)
    }

    pub fn select(&mut self, index: usize) {
        if index < self.panes.len() {
            self.active = index;
        }
    }

    pub fn next(&mut self) {
        if !self.panes.is_empty() {
            self.active = (self.active + 1) % self.panes.len();
        }
    }

    pub fn previous(&mut self) {
        if !self.panes.is_empty() {
            self.active = (self.active + self.panes.len() - 1) % self.panes.len();
        }
    }

    pub fn active(&self) -> Option<PaneId> {
        self.panes.get(self.active).copied()
    }

    pub fn active_index(&self) -> usize {
        self.active
    }

    pub fn panes(&self) -> &[PaneId] {
        &self.panes
    }

    pub fn is_empty(&self) -> bool {
        self.panes.is_empty()
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
    fn menutup_tab_aktif_memilih_tab_di_posisi_sama() {
        let mut tabs = tabs(3);
        let last = tabs.panes()[2];
        tabs.select(1);
        tabs.close(1);
        assert_eq!(tabs.active(), Some(last));
    }

    #[test]
    fn menutup_tab_aktif_paling_kanan_memilih_tab_sebelumnya() {
        let mut tabs = tabs(3);
        let middle = tabs.panes()[1];
        tabs.close(2);
        assert_eq!(tabs.active(), Some(middle));
    }

    #[test]
    fn menutup_tab_lain_tidak_mengubah_tab_aktif() {
        let mut tabs = tabs(3);
        let active = tabs.panes()[2];
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
        assert!(tabs.close(0).is_some());
        assert!(tabs.is_empty());
        assert_eq!(tabs.active(), None);
        assert_eq!(tabs.close(0), None);
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
    fn pindah_tanpa_tab_tidak_panik() {
        let mut tabs = Tabs::default();
        tabs.next();
        tabs.previous();
        tabs.select(3);
        assert_eq!(tabs.active(), None);
    }
}
