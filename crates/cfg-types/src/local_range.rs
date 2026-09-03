//! Positions measured from the start of a method, not of its file.
//!
//! A value computed for one method must not change when text before that
//! method moves — otherwise every per-method memo in the file is invalidated
//! by an edit in any other method. The lowered body therefore records
//! positions as [`LocalRange`] / [`LocalOffset`], relative to the method's own
//! syntax node, and only the boundary that knows where the method sits in the
//! file — a [`MethodOffset`] — can turn them back into file positions. Mixing
//! the two spaces is a type error, not a convention.

use text_size::{TextRange, TextSize};

/// Where a method's syntax node starts in its file; the sole bridge from
/// method-relative positions to file positions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct MethodOffset(TextSize);

impl MethodOffset {
    /// The file root itself: module-level code is lowered from the whole file,
    /// so its "local" positions already are file positions.
    pub const ZERO: MethodOffset = MethodOffset(TextSize::new(0));

    pub fn new(start: TextSize) -> Self {
        MethodOffset(start)
    }

    pub fn lift(self, range: LocalRange) -> TextRange {
        range.0 + self.0
    }

    pub fn lift_offset(self, offset: LocalOffset) -> TextSize {
        offset.0 + self.0
    }

    /// The method-relative form of a file range, or `None` when the range
    /// starts before the method — a lookup with such a range cannot hit.
    pub fn lower(self, range: TextRange) -> Option<LocalRange> {
        range.checked_sub(self.0).map(LocalRange)
    }

    pub fn lower_offset(self, offset: TextSize) -> Option<LocalOffset> {
        offset.checked_sub(self.0).map(LocalOffset)
    }
}

/// A position relative to the start of the owning method.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct LocalOffset(TextSize);

impl LocalOffset {
    pub fn lift(self, base: MethodOffset) -> TextSize {
        base.lift_offset(self)
    }
}

/// A range relative to the start of the owning method.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct LocalRange(TextRange);

impl LocalRange {
    /// A range read off a tree whose root is the method itself, so its
    /// offsets are already method-relative. This is the lowering side's
    /// constructor; reading a range off the file tree and passing it here
    /// silently produces a file position wearing the local type.
    pub fn of_detached_node(range: TextRange) -> Self {
        LocalRange(range)
    }

    pub fn empty(offset: LocalOffset) -> Self {
        LocalRange(TextRange::empty(offset.0))
    }

    pub fn start(self) -> LocalOffset {
        LocalOffset(self.0.start())
    }

    pub fn end(self) -> LocalOffset {
        LocalOffset(self.0.end())
    }

    pub fn len(self) -> TextSize {
        self.0.len()
    }

    pub fn is_empty(self) -> bool {
        self.0.is_empty()
    }

    pub fn contains(self, offset: LocalOffset) -> bool {
        self.0.contains(offset.0)
    }

    pub fn contains_inclusive(self, offset: LocalOffset) -> bool {
        self.0.contains_inclusive(offset.0)
    }

    pub fn contains_range(self, other: LocalRange) -> bool {
        self.0.contains_range(other.0)
    }

    pub fn intersect(self, other: LocalRange) -> Option<LocalRange> {
        self.0.intersect(other.0).map(LocalRange)
    }

    pub fn cover(self, other: LocalRange) -> LocalRange {
        LocalRange(self.0.cover(other.0))
    }

    /// The range in the coordinates of the body's own root — for slicing the
    /// detached root's text or walking its tree, never for the file.
    pub fn in_root(self) -> TextRange {
        self.0
    }

    pub fn lift(self, base: MethodOffset) -> TextRange {
        base.lift(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn range(start: u32, end: u32) -> TextRange {
        TextRange::new(TextSize::new(start), TextSize::new(end))
    }

    #[test]
    fn lift_and_lower_are_inverse_around_the_method_start() {
        let base = MethodOffset::new(TextSize::new(100));
        let local = LocalRange::of_detached_node(range(5, 9));
        assert_eq!(local.lift(base), range(105, 109));
        assert_eq!(base.lower(range(105, 109)), Some(local));
        assert_eq!(base.lower_offset(TextSize::new(105)), Some(local.start()));
    }

    #[test]
    fn a_range_before_the_method_has_no_local_form() {
        let base = MethodOffset::new(TextSize::new(100));
        assert_eq!(base.lower(range(90, 110)), None);
        assert_eq!(base.lower_offset(TextSize::new(99)), None);
    }

    #[test]
    fn the_file_root_lifts_by_nothing() {
        let local = LocalRange::of_detached_node(range(3, 7));
        assert_eq!(local.lift(MethodOffset::ZERO), range(3, 7));
    }

    #[test]
    fn local_geometry_mirrors_text_range() {
        let outer = LocalRange::of_detached_node(range(2, 10));
        let inner = LocalRange::of_detached_node(range(4, 6));
        assert!(outer.contains_range(inner));
        assert!(outer.contains(inner.start()));
        assert!(!inner.contains(outer.end()));
        assert!(inner.contains_inclusive(inner.end()));
        assert_eq!(outer.intersect(inner), Some(inner));
        assert_eq!(inner.cover(outer), outer);
        assert_eq!(inner.len(), TextSize::new(2));
        assert!(LocalRange::empty(inner.start()).is_empty());
        assert!(inner.start() < inner.end());
    }
}
