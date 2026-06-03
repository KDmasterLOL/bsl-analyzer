use smallvec::SmallVec;

use crate::{MethodId, Name, VariableId};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct QualifiedName {
    segments: SmallVec<[Name; 2]>,
}

impl QualifiedName {
    pub fn from_segments(segments: impl IntoIterator<Item = Name>) -> Self {
        Self { segments: segments.into_iter().collect() }
    }

    pub fn segments(&self) -> &[Name] {
        &self.segments
    }

    pub fn first(&self) -> &Name {
        &self.segments[0]
    }

    pub fn last(&self) -> &Name {
        self.segments.last().expect("QualifiedName must have at least one segment")
    }

    pub fn len(&self) -> usize {
        self.segments.len()
    }

    pub fn is_empty(&self) -> bool {
        self.segments.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathResolution {
    Method(MethodId),

    Variable(VariableId),

    Builtin(Name),

    Unresolved(QualifiedName),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_qualified_name_creation() {
        let name = QualifiedName::from_segments([Name::new("Module"), Name::new("Method")]);

        assert_eq!(name.len(), 2);
        assert_eq!(name.first(), &Name::new("Module"));
        assert_eq!(name.last(), &Name::new("Method"));
    }

    #[test]
    fn test_qualified_name_three_segments() {
        let name = QualifiedName::from_segments([
            Name::new("Документы"),
            Name::new("ПКО"),
            Name::new("Создать"),
        ]);

        assert_eq!(name.len(), 3);
        assert_eq!(name.segments()[0], Name::new("Документы"));
        assert_eq!(name.segments()[1], Name::new("ПКО"));
        assert_eq!(name.segments()[2], Name::new("Создать"));
    }

    #[test]
    fn test_qualified_name_preserves_case() {
        let name1 = QualifiedName::from_segments([Name::new("ОбщийМодуль"), Name::new("Метод")]);
        let name2 = QualifiedName::from_segments([Name::new("общиймодуль"), Name::new("метод")]);

        assert_ne!(name1, name2);

        let name3 = QualifiedName::from_segments([Name::new("ОбщийМодуль"), Name::new("Метод")]);
        assert_eq!(name1, name3);
    }
}
