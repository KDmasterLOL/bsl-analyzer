use serde::{Deserialize, Serialize};
use stdx::case::CaseExt;

use crate::metadata_object::{MdoType, Name};

/// A configuration subsystem (`Подсистема`) — an organisational container that lists the
/// metadata objects belonging to it and its directly-nested child subsystems. Subsystems
/// are not data-bearing objects (no manager, ref type, or attributes); they exist here so
/// the call graph can answer "which subsystems contain this object" for impact analysis.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Subsystem {
    name: String,
    /// Member metadata objects from `<Content>`, as the (type, name) pairs that parsed.
    content: Vec<(MdoType, Name)>,
    /// Names of directly-nested child subsystems from `<ChildObjects><Subsystem>`.
    child_subsystems: Vec<Name>,
}

impl Subsystem {
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into(), content: Vec::new(), child_subsystems: Vec::new() }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn content(&self) -> &[(MdoType, Name)] {
        &self.content
    }

    pub fn child_subsystems(&self) -> &[Name] {
        &self.child_subsystems
    }

    pub fn with_content(mut self, content: Vec<(MdoType, Name)>) -> Self {
        self.content = content;
        self
    }

    pub fn with_child_subsystems(mut self, children: Vec<Name>) -> Self {
        self.child_subsystems = children;
        self
    }

    /// Merge another subsystem's members and child subsystems into this one — an extension
    /// overlay that adds to a base subsystem of the same name. New entries are appended;
    pub fn merge_from(&mut self, other: &Subsystem) {
        for entry in &other.content {
            if !self.content.iter().any(|existing| {
                existing.0 == entry.0 && existing.1.fold_lower() == entry.1.fold_lower()
            }) {
                self.content.push(entry.clone());
            }
        }
        for child in &other.child_subsystems {
            if !self.child_subsystems.iter().any(|c| c.fold_lower() == child.fold_lower()) {
                self.child_subsystems.push(child.clone());
            }
        }
    }

    /// Heap bytes owned by this subsystem, memoised by `ide-db`'s
    /// `parse_subsystem_query` for Salsa's `heap_size` hook: its name plus the
    /// content and child-subsystem-name vecs. New heap-owning fields must be
    /// added here too.
    pub fn estimated_heap_size(&self) -> usize {
        self.name.capacity()
            + stdx::heap::vec_bytes::<(MdoType, Name)>(self.content.len())
            + self.content.iter().map(|(_, name)| name.capacity()).sum::<usize>()
            + stdx::heap::vec_bytes::<Name>(self.child_subsystems.len())
            + self.child_subsystems.iter().map(String::capacity).sum::<usize>()
    }
}
