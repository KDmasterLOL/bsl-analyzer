//! Normalized dependency topology of configuration extensions (CFE).
//!
//! The project config declares extensions either as bare path strings (legacy,
//! always independent) or as structured entries with a stable `name` and
//! directed `dependsOn` edges. This module owns the domain model those
//! declarations normalize into: validated nodes, a deterministic topological
//! order, per-node transitive dependency closures, and a byte-stable
//! fingerprint that changes whenever the topology's identity (paths, names,
//! edges, or order) changes.
//!
//! The model is deliberately independent of the configuration syntax so a
//! pre-validated graph from an external build tool can later be adopted
//! without touching consumers.

use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};

use stdx::case::fold_lower_per_char;

/// Bumped whenever the fingerprint recipe or the normalized model's semantics
/// change, so persisted identities derived from a fingerprint can never match
/// across incompatible formats.
pub const TOPOLOGY_FORMAT_VERSION: u32 = 1;

/// Index of a node inside [`ExtensionTopology`]; stable for the lifetime of
/// the topology value it came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeId(u32);

impl NodeId {
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// Raw material for one extension node, produced by the config resolver.
#[derive(Debug, Clone)]
pub struct ExtensionNodeSpec {
    pub name: String,
    /// Path as configured/expanded — what consumers watch and scan.
    pub path: PathBuf,
    /// Canonicalized identity path — what dedup and the fingerprint use.
    pub canonical_path: PathBuf,
    /// Direct dependency names as declared (`dependsOn`).
    pub depends_on: Vec<String>,
    /// Structured entries get strict validation (duplicate names are errors);
    /// legacy string entries keep their historical lenient semantics.
    pub structured: bool,
}

#[derive(Debug, Clone)]
pub struct ExtensionNode {
    name: String,
    path: PathBuf,
    canonical_path: PathBuf,
    depends_on: Vec<NodeId>,
    closure: Vec<NodeId>,
    structured: bool,
}

impl ExtensionNode {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn canonical_path(&self) -> &Path {
        &self.canonical_path
    }

    pub fn depends_on(&self) -> &[NodeId] {
        &self.depends_on
    }

    /// Ordered transitive dependencies (global topological order, diamond
    /// dependencies included once). The node itself is *not* part of its
    /// closure: visibility composition appends it last.
    pub fn closure(&self) -> &[NodeId] {
        &self.closure
    }

    pub fn is_structured(&self) -> bool {
        self.structured
    }
}

/// Byte-stable identity of a topology. Any change to the base path, node set,
/// node names, canonical paths, edges, or order produces a different value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TopologyFingerprint([u8; 32]);

impl TopologyFingerprint {
    pub fn to_hex(self) -> String {
        let mut out = String::with_capacity(64);
        for byte in self.0 {
            use fmt::Write as _;
            let _ = write!(out, "{byte:02x}");
        }
        out
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TopologyError {
    EmptyName,
    DuplicateName {
        name: String,
    },
    DuplicatePath {
        path: PathBuf,
        first: String,
        second: String,
    },
    UnknownDependency {
        from: String,
        name: String,
    },
    SelfReference {
        name: String,
    },
    DuplicateEdge {
        from: String,
        name: String,
    },
    /// The dependency name exists but is ambiguous: several legacy entries
    /// share it, so an edge cannot pick a target.
    AmbiguousDependency {
        from: String,
        name: String,
    },
    Cycle {
        path: Vec<String>,
    },
    GlobInStructuredEntry {
        name: String,
        pattern: String,
    },
    StructuredPathMissing {
        name: String,
        path: PathBuf,
    },
    StructuredNotAnExtension {
        name: String,
        path: PathBuf,
    },
}

impl fmt::Display for TopologyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TopologyError::EmptyName => {
                write!(f, "extension entry has an empty name")
            }
            TopologyError::DuplicateName { name } => {
                write!(f, "duplicate extension name: {name}")
            }
            TopologyError::DuplicatePath { path, first, second } => {
                write!(
                    f,
                    "extensions '{first}' and '{second}' resolve to the same path: {}",
                    path.display()
                )
            }
            TopologyError::UnknownDependency { from, name } => {
                write!(f, "extension '{from}' depends on unknown extension '{name}'")
            }
            TopologyError::SelfReference { name } => {
                write!(f, "extension '{name}' depends on itself")
            }
            TopologyError::DuplicateEdge { from, name } => {
                write!(f, "extension '{from}' declares a duplicate dependency on '{name}'")
            }
            TopologyError::AmbiguousDependency { from, name } => {
                write!(
                    f,
                    "extension '{from}' depends on '{name}', which several extensions share as a name"
                )
            }
            TopologyError::Cycle { path } => {
                write!(f, "extension dependency cycle: {}", path.join(" -> "))
            }
            TopologyError::GlobInStructuredEntry { name, pattern } => {
                write!(
                    f,
                    "extension '{name}': glob patterns are not allowed in a named entry's path: {pattern}"
                )
            }
            TopologyError::StructuredPathMissing { name, path } => {
                write!(f, "extension '{name}': path not found: {}", path.display())
            }
            TopologyError::StructuredNotAnExtension { name, path } => {
                write!(f, "extension '{name}': no Configuration.xml under {}", path.display())
            }
        }
    }
}

impl std::error::Error for TopologyError {}

#[derive(Debug, Clone)]
pub struct ExtensionTopology {
    /// Nodes in expanded declaration order — the order `extension_paths()`
    /// exposes and overlay composition relies on.
    nodes: Vec<ExtensionNode>,
    /// Dependencies-before-dependents order; ties broken by declaration order.
    topo_order: Vec<NodeId>,
    fingerprint: TopologyFingerprint,
}

impl ExtensionTopology {
    /// Validates the specs and builds the normalized model. `base_path` is the
    /// canonical base-configuration root, folded into the fingerprint because
    /// the same extension set over a different base is a different project.
    pub fn build(
        base_path: &Path,
        specs: Vec<ExtensionNodeSpec>,
    ) -> Result<ExtensionTopology, TopologyError> {
        // Name table. Legacy entries may share a (derived) name — historical
        // behavior that stays warning-only as long as nothing depends on the
        // ambiguous name. Structured entries must be unique outright.
        let mut by_name: HashMap<String, NameSlot> = HashMap::new();
        for (idx, spec) in specs.iter().enumerate() {
            if spec.name.trim().is_empty() {
                return Err(TopologyError::EmptyName);
            }
            let key = fold_lower_per_char(&spec.name);
            match by_name.entry(key) {
                std::collections::hash_map::Entry::Vacant(entry) => {
                    entry.insert(NameSlot::Unique(idx));
                }
                std::collections::hash_map::Entry::Occupied(mut entry) => {
                    let first_structured = match *entry.get() {
                        NameSlot::Unique(first) => specs[first].structured,
                        NameSlot::Ambiguous => false,
                    };
                    if spec.structured || first_structured {
                        return Err(TopologyError::DuplicateName { name: spec.name.clone() });
                    }
                    tracing::warn!(
                        name = %spec.name,
                        "several legacy extension entries share a name; \
                         they stay independent and cannot be dependency targets"
                    );
                    entry.insert(NameSlot::Ambiguous);
                }
            }
        }

        // Resolve declared edges to node ids.
        let mut edges: Vec<Vec<NodeId>> = Vec::with_capacity(specs.len());
        for (idx, spec) in specs.iter().enumerate() {
            let mut resolved: Vec<NodeId> = Vec::with_capacity(spec.depends_on.len());
            for dep_name in &spec.depends_on {
                let key = fold_lower_per_char(dep_name);
                let target = match by_name.get(&key) {
                    None => {
                        return Err(TopologyError::UnknownDependency {
                            from: spec.name.clone(),
                            name: dep_name.clone(),
                        })
                    }
                    Some(NameSlot::Ambiguous) => {
                        return Err(TopologyError::AmbiguousDependency {
                            from: spec.name.clone(),
                            name: dep_name.clone(),
                        })
                    }
                    Some(NameSlot::Unique(target)) => *target,
                };
                if target == idx {
                    return Err(TopologyError::SelfReference { name: spec.name.clone() });
                }
                let id = NodeId(target as u32);
                if resolved.contains(&id) {
                    return Err(TopologyError::DuplicateEdge {
                        from: spec.name.clone(),
                        name: dep_name.clone(),
                    });
                }
                resolved.push(id);
            }
            edges.push(resolved);
        }

        let topo_order = topological_order(&specs, &edges)?;

        // Transitive closures in topological order: a node's closure is the
        // union of its direct dependencies and their closures, ordered by the
        // global topological rank, so a diamond contributes each node once.
        let mut rank = vec![0usize; specs.len()];
        for (pos, id) in topo_order.iter().enumerate() {
            rank[id.index()] = pos;
        }
        let mut closures: Vec<Vec<NodeId>> = vec![Vec::new(); specs.len()];
        for id in &topo_order {
            let mut closure: Vec<NodeId> = Vec::new();
            for dep in &edges[id.index()] {
                for transitive in &closures[dep.index()] {
                    if !closure.contains(transitive) {
                        closure.push(*transitive);
                    }
                }
                if !closure.contains(dep) {
                    closure.push(*dep);
                }
            }
            closure.sort_by_key(|n| rank[n.index()]);
            closures[id.index()] = closure;
        }

        let nodes: Vec<ExtensionNode> = specs
            .into_iter()
            .zip(edges)
            .zip(closures)
            .map(|((spec, depends_on), closure)| ExtensionNode {
                name: spec.name,
                path: spec.path,
                canonical_path: spec.canonical_path,
                depends_on,
                closure,
                structured: spec.structured,
            })
            .collect();

        let fingerprint = fingerprint(base_path, &nodes);
        Ok(ExtensionTopology { nodes, topo_order, fingerprint })
    }

    /// Nodes in expanded declaration order.
    pub fn nodes(&self) -> &[ExtensionNode] {
        &self.nodes
    }

    pub fn node(&self, id: NodeId) -> &ExtensionNode {
        &self.nodes[id.index()]
    }

    pub fn node_ids(&self) -> impl Iterator<Item = NodeId> + '_ {
        (0..self.nodes.len() as u32).map(NodeId)
    }

    /// Dependencies-before-dependents; ties follow declaration order, so a
    /// graph without edges keeps the declared order exactly.
    pub fn topological_order(&self) -> &[NodeId] {
        &self.topo_order
    }

    pub fn fingerprint(&self) -> TopologyFingerprint {
        self.fingerprint
    }

    pub fn has_dependencies(&self) -> bool {
        self.nodes.iter().any(|node| !node.depends_on.is_empty())
    }
}

#[derive(Clone, Copy)]
enum NameSlot {
    Unique(usize),
    Ambiguous,
}

/// Kahn's algorithm with a deterministic tie-breaker: among ready nodes the
/// smallest declaration index goes first. On a cycle, walks the leftover nodes
/// to report one concrete cycle path.
fn topological_order(
    specs: &[ExtensionNodeSpec],
    edges: &[Vec<NodeId>],
) -> Result<Vec<NodeId>, TopologyError> {
    let n = specs.len();
    let mut remaining_deps: Vec<usize> = edges.iter().map(Vec::len).collect();
    let mut dependents: Vec<Vec<usize>> = vec![Vec::new(); n];
    for (idx, deps) in edges.iter().enumerate() {
        for dep in deps {
            dependents[dep.index()].push(idx);
        }
    }

    let mut ready: std::collections::BTreeSet<usize> =
        (0..n).filter(|&idx| remaining_deps[idx] == 0).collect();
    let mut order: Vec<NodeId> = Vec::with_capacity(n);
    while let Some(idx) = ready.pop_first() {
        order.push(NodeId(idx as u32));
        for &dependent in &dependents[idx] {
            remaining_deps[dependent] -= 1;
            if remaining_deps[dependent] == 0 {
                ready.insert(dependent);
            }
        }
    }

    if order.len() == n {
        return Ok(order);
    }

    // Every leftover node sits on or downstream of a cycle. Walking first
    // unvisited dependencies from the smallest leftover index must revisit a
    // node — that revisit closes the reported cycle.
    let leftover: Vec<usize> = (0..n).filter(|&idx| remaining_deps[idx] > 0).collect();
    let start = leftover[0];
    let mut seen_at: HashMap<usize, usize> = HashMap::new();
    let mut path: Vec<usize> = Vec::new();
    let mut current = start;
    loop {
        if let Some(&pos) = seen_at.get(&current) {
            let mut cycle: Vec<String> =
                path[pos..].iter().map(|&idx| specs[idx].name.clone()).collect();
            cycle.push(specs[current].name.clone());
            return Err(TopologyError::Cycle { path: cycle });
        }
        seen_at.insert(current, path.len());
        path.push(current);
        current = edges[current]
            .iter()
            .map(|id| id.index())
            .find(|idx| remaining_deps[*idx] > 0)
            .expect("a node left by Kahn's algorithm keeps an unresolved dependency");
    }
}

/// Domain-separated, versioned, length-prefixed digest over the normalized
/// model. Names enter case-folded (the identity the resolver compares by);
/// paths enter as their platform-encoded bytes (injective, unlike a lossy
/// UTF-8 conversion). Nodes hash in declaration order, which — together with
/// the per-node edges — fully determines the derived topological order, so
/// reordering declarations or touching any edge changes the digest: declared
/// order is part of overlay precedence and therefore of project identity.
fn fingerprint(base_path: &Path, nodes: &[ExtensionNode]) -> TopologyFingerprint {
    let mut hasher = blake3::Hasher::new_derive_key("bsl-analyzer/extension-topology/v1");
    let field = |hasher: &mut blake3::Hasher, bytes: &[u8]| {
        hasher.update(&(bytes.len() as u64).to_le_bytes());
        hasher.update(bytes);
    };
    hasher.update(&TOPOLOGY_FORMAT_VERSION.to_le_bytes());
    field(&mut hasher, base_path.as_os_str().as_encoded_bytes());
    hasher.update(&(nodes.len() as u64).to_le_bytes());
    for node in nodes {
        field(&mut hasher, fold_lower_per_char(&node.name).as_bytes());
        field(&mut hasher, node.canonical_path.as_os_str().as_encoded_bytes());
        let mut deps: Vec<String> = node
            .depends_on
            .iter()
            .map(|dep| fold_lower_per_char(&nodes[dep.index()].name))
            .collect();
        deps.sort();
        hasher.update(&(deps.len() as u64).to_le_bytes());
        for dep in &deps {
            field(&mut hasher, dep.as_bytes());
        }
    }
    TopologyFingerprint(*hasher.finalize().as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(name: &str, deps: &[&str]) -> ExtensionNodeSpec {
        ExtensionNodeSpec {
            name: name.to_string(),
            path: PathBuf::from(format!("/ws/{name}")),
            canonical_path: PathBuf::from(format!("/ws/{name}")),
            depends_on: deps.iter().map(|s| s.to_string()).collect(),
            structured: true,
        }
    }

    fn legacy_spec(name: &str) -> ExtensionNodeSpec {
        ExtensionNodeSpec { structured: false, ..spec(name, &[]) }
    }

    fn build(specs: Vec<ExtensionNodeSpec>) -> Result<ExtensionTopology, TopologyError> {
        ExtensionTopology::build(Path::new("/ws/base"), specs)
    }

    fn names(topology: &ExtensionTopology, ids: &[NodeId]) -> Vec<String> {
        ids.iter().map(|id| topology.node(*id).name().to_string()).collect()
    }

    #[test]
    fn no_edges_keeps_declaration_order() {
        let topology = build(vec![spec("B", &[]), spec("A", &[]), legacy_spec("C")]).unwrap();
        let order = names(&topology, topology.topological_order());
        assert_eq!(order, ["B", "A", "C"], "without edges the declared order is the topo order");
        for node in topology.nodes() {
            assert!(node.closure().is_empty());
        }
    }

    #[test]
    fn chain_closure_lists_transitive_dependencies_in_order() {
        let topology = build(vec![
            spec("TESTS", &["yaxunit"]),
            spec("yaxunit", &[]),
            spec("INDEPENDENT", &[]),
        ])
        .unwrap();
        let tests = &topology.nodes()[0];
        assert_eq!(names(&topology, tests.closure()), ["yaxunit"]);
        let independent = &topology.nodes()[2];
        assert!(independent.closure().is_empty());
        assert_eq!(
            names(&topology, topology.topological_order()),
            ["yaxunit", "TESTS", "INDEPENDENT"],
            "a dependency moves ahead of its dependent; unrelated nodes keep declared order"
        );
    }

    #[test]
    fn diamond_dependency_enters_closure_once() {
        let topology = build(vec![
            spec("A", &["B", "C"]),
            spec("B", &["D"]),
            spec("C", &["D"]),
            spec("D", &[]),
        ])
        .unwrap();
        let a = &topology.nodes()[0];
        assert_eq!(names(&topology, a.closure()), ["D", "B", "C"]);
    }

    #[test]
    fn direct_cycle_reports_full_path() {
        let err =
            build(vec![spec("TESTS", &["yaxunit"]), spec("yaxunit", &["TESTS"])]).unwrap_err();
        assert_eq!(err.to_string(), "extension dependency cycle: TESTS -> yaxunit -> TESTS");
    }

    #[test]
    fn transitive_cycle_reports_full_path() {
        let err = build(vec![spec("A", &["B"]), spec("B", &["C"]), spec("C", &["A"])]).unwrap_err();
        assert_eq!(err.to_string(), "extension dependency cycle: A -> B -> C -> A");
    }

    #[test]
    fn unknown_dependency_is_rejected() {
        let err = build(vec![spec("TESTS", &["missing"])]).unwrap_err();
        assert_eq!(
            err,
            TopologyError::UnknownDependency { from: "TESTS".into(), name: "missing".into() }
        );
    }

    #[test]
    fn self_reference_is_rejected_case_insensitively() {
        let err = build(vec![spec("Тесты", &["ТЕСТЫ"])]).unwrap_err();
        assert_eq!(err, TopologyError::SelfReference { name: "Тесты".into() });
    }

    #[test]
    fn duplicate_edge_is_rejected_case_insensitively() {
        let err = build(vec![spec("A", &["Base", "base"]), spec("Base", &[])]).unwrap_err();
        assert_eq!(err, TopologyError::DuplicateEdge { from: "A".into(), name: "base".into() });
    }

    #[test]
    fn duplicate_structured_name_is_rejected_case_insensitively() {
        let err = build(vec![spec("Тесты", &[]), spec("ТЕСТЫ", &[])]).unwrap_err();
        assert_eq!(err, TopologyError::DuplicateName { name: "ТЕСТЫ".into() });
    }

    #[test]
    fn legacy_duplicate_names_stay_but_cannot_be_targets() {
        let topology =
            build(vec![legacy_spec("Ext"), legacy_spec("Ext")]).expect("legacy duplicates stay");
        assert_eq!(topology.nodes().len(), 2);

        let err =
            build(vec![legacy_spec("Ext"), legacy_spec("Ext"), spec("T", &["Ext"])]).unwrap_err();
        assert_eq!(
            err,
            TopologyError::AmbiguousDependency { from: "T".into(), name: "Ext".into() }
        );
    }

    #[test]
    fn empty_name_is_rejected() {
        let err = build(vec![spec("  ", &[])]).unwrap_err();
        assert_eq!(err, TopologyError::EmptyName);
    }

    #[test]
    fn fingerprint_is_stable_and_tracks_identity() {
        let base = || vec![spec("TESTS", &["yaxunit"]), spec("yaxunit", &[]), spec("IND", &[])];
        let fp = |specs| build(specs).unwrap().fingerprint();

        assert_eq!(fp(base()), fp(base()), "identical inputs must produce identical digests");

        let mut renamed = base();
        renamed[2].name = "IND2".to_string();
        assert_ne!(fp(base()), fp(renamed));

        let mut repathed = base();
        repathed[2].canonical_path = PathBuf::from("/elsewhere/IND");
        assert_ne!(fp(base()), fp(repathed));

        let mut extra_edge = base();
        extra_edge[2].depends_on.push("yaxunit".to_string());
        assert_ne!(fp(base()), fp(extra_edge));

        let reordered = vec![spec("IND", &[]), spec("TESTS", &["yaxunit"]), spec("yaxunit", &[])];
        assert_ne!(fp(base()), fp(reordered), "declared order is part of overlay identity");

        let other_base =
            ExtensionTopology::build(Path::new("/ws/other-base"), base()).unwrap().fingerprint();
        assert_ne!(fp(base()), other_base, "the base root is part of project identity");
    }

    #[test]
    fn fingerprint_distinguishes_declaration_order_even_with_equal_topo_order() {
        let a_first = build(vec![spec("A", &["B"]), spec("B", &[])]).unwrap();
        let b_first = build(vec![spec("B", &[]), spec("A", &["B"])]).unwrap();
        assert_eq!(
            names(&a_first, a_first.topological_order()),
            names(&b_first, b_first.topological_order()),
            "both declarations topo-sort to the same B -> A order"
        );
        assert_ne!(
            a_first.fingerprint(),
            b_first.fingerprint(),
            "declaration order drives overlay precedence and must change the digest"
        );
    }

    #[test]
    fn fingerprint_golden_vector() {
        let topology = build(vec![spec("TESTS", &["yaxunit"]), spec("yaxunit", &[])]).unwrap();
        expect_test::expect!["c3d6c675c346fbcd6703281b8e437b3dfa2b3e54cc9460e4a5451a1bb9cf275b"]
            .assert_eq(&topology.fingerprint().to_hex());
    }
}
