//! Values a processor measures for its display: a tuner's pitch, a
//! compressor's gain reduction, a gate's state.
//!
//! Parameters flow from the UI to the audio thread; a readout flows the
//! other way. The processor writes its cells from `process` and whoever
//! drives the display reads them at display rate, through a clone of
//! the same [`Readout`]. Each cell is one `f32` behind an atomic, so a
//! write never blocks and a read sees a whole value, never half of one.
//! The shell streams every readout in a running graph to the UI in the
//! same frame as the meters (see `docs/frames.md`).

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

/// A processor's display values, shared between the audio thread that
/// writes them and the display thread that reads them. Cloning shares
/// the cells.
#[derive(Debug, Clone)]
pub struct Readout {
    names: &'static [&'static str],
    cells: Arc<[AtomicU32]>,
}

impl Readout {
    /// A readout with one cell per name, all zero.
    pub fn new(names: &'static [&'static str]) -> Self {
        Self {
            names,
            cells: names.iter().map(|_| AtomicU32::new(0)).collect(),
        }
    }

    /// What each cell holds, in cell order.
    pub fn names(&self) -> &'static [&'static str] {
        self.names
    }

    pub fn len(&self) -> usize {
        self.cells.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }

    /// Writes a cell. Real-time safe. Unknown indexes are ignored.
    #[inline]
    pub fn set(&self, index: usize, value: f32) {
        if let Some(cell) = self.cells.get(index) {
            cell.store(value.to_bits(), Ordering::Relaxed);
        }
    }

    /// Reads a cell. Unknown indexes read zero.
    #[inline]
    pub fn get(&self, index: usize) -> f32 {
        self.cells
            .get(index)
            .map_or(0.0, |c| f32::from_bits(c.load(Ordering::Relaxed)))
    }

    /// Every cell, in order.
    pub fn values(&self) -> impl Iterator<Item = f32> + '_ {
        (0..self.cells.len()).map(|i| self.get(i))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clones_share_cells() {
        let a = Readout::new(&["Pitch", "Clarity"]);
        let b = a.clone();
        a.set(0, 440.0);
        a.set(1, 0.9);
        assert_eq!(b.get(0), 440.0);
        assert_eq!(b.get(1), 0.9);
        assert_eq!(b.names(), &["Pitch", "Clarity"]);
        assert_eq!(b.values().collect::<Vec<_>>(), vec![440.0, 0.9]);
    }

    #[test]
    fn unknown_cells_are_ignored() {
        let r = Readout::new(&["One"]);
        r.set(5, 1.0);
        assert_eq!(r.get(5), 0.0);
        assert_eq!(r.len(), 1);
    }
}
