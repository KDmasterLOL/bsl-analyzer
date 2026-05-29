#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Progress {
    Begin,
    Report,
    End,
}

impl Progress {
    pub fn fraction(done: usize, total: usize) -> f64 {
        assert!(done <= total);
        done as f64 / total.max(1) as f64
    }
}
