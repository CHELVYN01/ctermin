//! Tree split pane: tiap daun adalah satu panel, tiap cabang membagi ruang menjadi dua.

use eframe::egui::{pos2, vec2, Pos2, Rect};

use super::PaneId;

const DEFAULT_RATIO: f32 = 0.5;
const MIN_PANE_SIZE: f32 = 48.0;
const EPSILON: f32 = 0.001;

/// Tebal pembatas di antara dua panel.
pub const DIVIDER_THICKNESS: f32 = 2.0;

/// Arah pembagian ruang.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Axis {
    /// Panel berdampingan kiri dan kanan.
    Horizontal,
    /// Panel bertumpuk atas dan bawah.
    Vertical,
}

/// Arah pindah fokus.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
}

/// Cabang pada jalur dari akar tree menuju sebuah split.
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

/// Pembatas yang bisa di-drag, beserta split yang diaturnya.
#[derive(Debug)]
pub struct Divider {
    pub path: Vec<Branch>,
    pub axis: Axis,
    pub rect: Rect,
    parent: Rect,
}

impl Divider {
    /// Rasio split agar pembatas berada di posisi pointer, tanpa membuat panel terlalu kecil.
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
    /// Membagi panel `target`. Panel baru berada di kanan atau bawah.
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

    /// Menghapus panel; saudaranya mengambil seluruh ruang induknya. `None` jika tree kosong.
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

    /// Panel yang sebaiknya menerima fokus setelah `target` ditutup.
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

    /// Kotak layar tiap panel di dalam `rect`.
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

    /// Panel terdekat dari `from` ke arah tertentu. Jika ada beberapa, dipilih yang paling
    /// banyak bersinggungan.
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

/// Ruang acuan untuk perhitungan yang hanya bergantung pada proporsi.
fn unit_rect() -> Rect {
    Rect::from_min_size(Pos2::ZERO, vec2(1000.0, 1000.0))
}

/// Jarak ke kotak lain pada arah tertentu dan panjang sisi yang bersinggungan.
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

/// Membagi kotak menjadi bagian pertama, pembatas, dan bagian kedua.
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

    /// Empat panel: A di kiri, B di kanan atas, lalu C dan D di kanan bawah.
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
    fn split_membagi_dua_dengan_panel_baru_di_kanan() {
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
    fn split_atas_bawah() {
        let [a, b] = ids();
        let mut layout = Layout::Leaf(a);
        layout.split(a, b, Axis::Vertical);
        let rects = layout.arrange(screen(), DIVIDER_THICKNESS);
        assert!(rects[0].1.bottom() < rects[1].1.top());
        assert_eq!(rects[0].1.width(), 800.0);
    }

    #[test]
    fn split_target_tidak_ada_tidak_mengubah_layout() {
        let [a, b, c] = ids();
        let mut layout = Layout::Leaf(a);
        assert!(!layout.split(b, c, Axis::Horizontal));
        assert_eq!(layout, Layout::Leaf(a));
    }

    #[test]
    fn split_bertingkat_mengisi_seluruh_ruang_tanpa_tumpang_tindih() {
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
                assert!(!overlap.is_positive(), "{first:?} menimpa {second:?}");
            }
        }
    }

    #[test]
    fn menutup_panel_memberi_ruangnya_ke_saudara() {
        let [a, b, c] = ids();
        let mut layout = Layout::Leaf(a);
        layout.split(a, b, Axis::Horizontal);
        layout.split(b, c, Axis::Vertical);

        let layout = layout.remove(c).expect("masih ada panel");
        let rects = layout.arrange(screen(), DIVIDER_THICKNESS);
        assert_eq!(rects.len(), 2);
        assert_eq!(rects[1].0, b);
        assert_eq!(rects[1].1.height(), 600.0, "b mengambil seluruh tinggi");
    }

    #[test]
    fn menutup_panel_di_tengah_tidak_merusak_sisanya() {
        let (layout, [a, b, c, d]) = four_panes();
        let layout = layout.remove(b).expect("masih ada panel");
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
    fn menutup_panel_terakhir_mengosongkan_tree() {
        let [a] = ids();
        assert_eq!(Layout::Leaf(a).remove(a), None);
    }

    #[test]
    fn saudara_panel_untuk_fokus_berikutnya() {
        let (layout, [a, b, c, d]) = four_panes();
        assert_eq!(layout.sibling_of(c), Some(d));
        assert_eq!(layout.sibling_of(d), Some(c));
        assert_eq!(layout.sibling_of(b), Some(c));
        assert_eq!(layout.sibling_of(a), Some(b));
        assert_eq!(Layout::Leaf(a).sibling_of(a), None);
    }

    #[test]
    fn tetangga_mengikuti_posisi_di_layar() {
        let (layout, [a, b, c, d]) = four_panes();
        assert_eq!(layout.neighbor(a, Direction::Right), Some(b));
        assert_eq!(layout.neighbor(b, Direction::Left), Some(a));
        assert_eq!(layout.neighbor(c, Direction::Left), Some(a));
        assert_eq!(layout.neighbor(c, Direction::Right), Some(d));
        assert_eq!(layout.neighbor(d, Direction::Up), Some(b));
        assert_eq!(layout.neighbor(b, Direction::Down), Some(c));
    }

    #[test]
    fn tidak_ada_tetangga_di_tepi() {
        let (layout, [a, b, _, d]) = four_panes();
        assert_eq!(layout.neighbor(a, Direction::Left), None);
        assert_eq!(layout.neighbor(a, Direction::Up), None);
        assert_eq!(layout.neighbor(b, Direction::Up), None);
        assert_eq!(layout.neighbor(d, Direction::Right), None);
        assert_eq!(layout.neighbor(d, Direction::Down), None);
    }

    #[test]
    fn rasio_mengikuti_pointer_dengan_batas_ukuran_panel() {
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
    fn rasio_aman_saat_ruang_sangat_kecil() {
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
    fn jalur_mengubah_rasio_split_yang_tepat() {
        let (mut layout, [_, b, c, d]) = four_panes();
        let dividers = layout.dividers(screen());
        let nested = dividers
            .iter()
            .find(|divider| divider.path == [Branch::Second, Branch::Second])
            .expect("split C dan D harus punya pembatas");
        let path = nested.path.clone();

        layout.set_ratio(&path, 0.25);

        let rects = layout.arrange(screen(), DIVIDER_THICKNESS);
        let rect = |pane| rects.iter().find(|(id, _)| *id == pane).map(|(_, r)| *r);
        let (b, c, d) = (rect(b), rect(c), rect(d));
        assert!(c.expect("c ada").width() < d.expect("d ada").width() / 2.0);
        assert!(
            b.expect("b ada").width() > 300.0,
            "split lain tidak berubah"
        );
    }
}
