//! Where each metadata object is defined, for the graph build.
//!
//! The rules that turn a directory tree into objects live once, in
//! `bsl_metadata`'s discovery. This only points them at the tree the build
//! already scanned — never at a fresh walk, which can see a tree the build did
//! not — and keys the result the way the graph keys an object.

use std::path::Path;

use bsl_conventions::DirTree;
use bsl_metadata::MdoType;
use ide::graph_index::{mdo_files_key, MdoFiles};

/// The file each metadata object is defined by, for every config root.
///
/// Base first: an object defined in the base configuration and extended by an
/// extension is ONE node in the graph, and its durable id belongs to the file
/// that defines it. The collision is decided on the FOLDED key, because that is
/// the only key on which a base `товары` and an extension `Товары` meet at all.
///
/// Only families whose kind exists as an [`MdoType`] are collected: the graph
/// can emit an `Mdo` node for no other kind, so a defined type or a scheduled
/// job has nothing to key on. A kind that is discovered but never emitted costs
/// only a map entry nobody reads.
/// The roots are taken CANONICALIZED, because the tree this reads is the scanned
/// universe and the universe stores canonical paths. A root spelled through a
/// symlink would share no prefix with them and discover nothing at all.
pub(crate) fn mdo_files(configs: &ide::WorkspaceConfigsSnapshot, tree: &dyn DirTree) -> MdoFiles {
    // A snapshot registered without canonicalization carries no counterpart; its
    // declared paths are then all there is. Pairing the two lists by position is
    // only valid while they are the same length.
    let canonical = (configs.canonical_paths.len() == configs.paths.len())
        .then_some(configs.canonical_paths.as_slice());

    let roots: Vec<(bool, &Path)> = configs
        .paths
        .iter()
        .enumerate()
        .map(|(i, (label, declared))| {
            let root = canonical.map_or(declared.as_path(), |paths| paths[i].as_path());
            (label.is_none(), root)
        })
        .collect();

    let mut out = MdoFiles::default();
    for (_, root) in roots.iter().filter(|(is_base, _)| *is_base) {
        collect_root(root, tree, &mut out);
    }
    for (_, root) in roots.iter().filter(|(is_base, _)| !*is_base) {
        collect_root(root, tree, &mut out);
    }
    out
}

fn collect_root(root: &Path, tree: &dyn DirTree, out: &mut MdoFiles) {
    let mut put = |mdo_type: MdoType, name: &str, file: &Path| {
        out.entry(mdo_files_key(mdo_type, name)).or_insert_with(|| encoded_path(file));
    };

    for mdo in bsl_metadata::discover_metadata_structure(root, tree) {
        put(mdo.mdo_type, &mdo.name, &mdo.main);
    }
    for register in bsl_metadata::discover_register_structure(root, tree) {
        put(register.mdo_type, &register.name, &register.main);
    }
    for module in bsl_metadata::discover_common_module_structure(root, tree) {
        // The body is the useful destination and the one the listing names; the
        // XML is the fallback for a module with no readable `.bsl`. Naming the
        // XML instead would leave the graph's row and the listing's row on
        // different files, which is the split this map exists to close.
        put(
            MdoType::CommonModule,
            &module.name,
            module.module_file.as_ref().unwrap_or(&module.main),
        );
    }
    for role in bsl_metadata::discover_role_structure(root, tree) {
        put(MdoType::Role, &role.name, &role.main);
    }
    for subsystem in bsl_metadata::discover_subsystem_structure(root, tree) {
        put(MdoType::Subsystem, &subsystem.name, &subsystem.main);
    }
    for subscription in bsl_metadata::discover_event_subscription_structure(root, tree) {
        put(MdoType::EventSubscription, &subscription.name, &subscription.main);
    }
}

/// The shape `GraphRowEncoder::path_for` produces for a module: absolute, with
/// separators normalised. A row whose file is spelled any other way is not the
/// path the name dictionary resolves against.
fn encoded_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use bsl_conventions::PathSetTree;
    use std::path::PathBuf;

    fn tree(paths: &[&str]) -> PathSetTree {
        PathSetTree::from_files(paths.iter().map(PathBuf::from))
    }

    fn configs(roots: &[(Option<&str>, &str)]) -> ide::WorkspaceConfigsSnapshot {
        // `from_paths` canonicalizes and keeps the declared path when that
        // fails, which is what these synthetic roots need.
        ide::WorkspaceConfigsSnapshot::from_paths(
            roots
                .iter()
                .map(|(label, path)| (label.map(str::to_string), PathBuf::from(path)))
                .collect(),
        )
    }

    fn file_of(map: &MdoFiles, mdo_type: MdoType, name: &str) -> Option<String> {
        map.get(&mdo_files_key(mdo_type, name)).cloned()
    }

    /// An object the base defines and an extension extends is one node, and its
    /// file is the base's.
    ///
    /// Two inputs carry the weight here, and neither is decoration.
    ///
    /// The roots spell the object differently, so the collision exists at all —
    /// with one spelling the keys collide folded or not, and base-wins reads as
    /// kept while being absent. And the BASE's spelling differs from its own
    /// folded form, so a builder that skipped folding would file the base under
    /// a key no lookup reaches: without that, the base happens to sit under its
    /// folded spelling and an unfolded map answers correctly by accident.
    #[test]
    fn the_base_root_wins_a_folded_collision() {
        // The object's XML is the SIBLING of its directory, as the designer
        // writes it.
        let tree = tree(&[
            "/ws/cf/Catalogs/ТОВАРЫ.xml",
            "/ws/cf/Catalogs/ТОВАРЫ/Ext/ObjectModule.bsl",
            "/ws/cfe/Catalogs/Товары.xml",
        ]);
        let map = mdo_files(&configs(&[(Some("Расш"), "/ws/cfe"), (None, "/ws/cf")]), &tree);

        for spelling in ["Товары", "товары", "ТОВАРЫ"] {
            assert_eq!(
                file_of(&map, MdoType::Catalog, spelling).as_deref(),
                Some("/ws/cf/Catalogs/ТОВАРЫ.xml"),
                "the object is defined in the base; the extension only adds to it \
                 — and the node may carry any of these spellings",
            );
        }
    }

    /// A common module is reached by its body — the file its resident record
    /// names — so the two rows land on one identity.
    #[test]
    fn a_common_module_is_keyed_to_its_body() {
        let tree = tree(&[
            "/ws/cf/CommonModules/Настройки.xml",
            "/ws/cf/CommonModules/Настройки/Ext/Module.bsl",
        ]);
        let map = mdo_files(&configs(&[(None, "/ws/cf")]), &tree);

        assert_eq!(
            file_of(&map, MdoType::CommonModule, "настройки").as_deref(),
            Some("/ws/cf/CommonModules/Настройки/Ext/Module.bsl"),
        );
    }

    /// A protected module has no body, and its XML is what remains.
    #[test]
    fn a_module_without_a_body_falls_back_to_its_xml() {
        let tree = tree(&["/ws/cf/CommonModules/Защищенный.xml"]);
        let map = mdo_files(&configs(&[(None, "/ws/cf")]), &tree);

        assert_eq!(
            file_of(&map, MdoType::CommonModule, "Защищенный").as_deref(),
            Some("/ws/cf/CommonModules/Защищенный.xml"),
        );
    }

    /// Roles and subsystems are the families that exist only to be graph nodes;
    /// leaving them out would place the objects and not the containers.
    #[test]
    fn containers_are_keyed_too() {
        let tree = tree(&[
            "/ws/cf/Roles/ПолныеПрава.xml",
            "/ws/cf/Subsystems/Продажи.xml",
            "/ws/cf/EventSubscriptions/ПриЗаписи.xml",
        ]);
        let map = mdo_files(&configs(&[(None, "/ws/cf")]), &tree);

        assert!(file_of(&map, MdoType::Role, "полныеправа").is_some());
        assert!(file_of(&map, MdoType::Subsystem, "продажи").is_some());
        assert!(file_of(&map, MdoType::EventSubscription, "призаписи").is_some());
    }
}
