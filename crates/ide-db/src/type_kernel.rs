use bsl_types::intern::{canonicalise, TypeKernelDb};
use bsl_types::kind::{TypeId, TypeKind};
use elsa::sync::FrozenVec;
use parking_lot::{RwLock, RwLockUpgradableReadGuard};
use rustc_hash::FxHashMap;

use crate::database::RootDatabaseImpl;

pub struct TypeKernelInner {
    table: FrozenVec<Box<TypeKind>>,
    intern: RwLock<FxHashMap<TypeKind, u64>>,
    sentinels: Sentinels,
}

#[derive(Debug, Clone, Copy)]
struct Sentinels {
    unknown: u64,
    never: u64,
    any: u64,
    null: u64,
    undefined: u64,
    boolean: u64,
}

impl Default for TypeKernelInner {
    fn default() -> Self {
        Self::new()
    }
}

impl TypeKernelInner {
    pub fn new() -> Self {
        let table = FrozenVec::new();
        let mut intern = FxHashMap::default();

        let mut seed = |kind: TypeKind| -> u64 {
            let id = table.len() as u64;
            table.push(Box::new(kind.clone()));
            intern.insert(kind, id);
            id
        };

        let sentinels = Sentinels {
            unknown: seed(TypeKind::Unknown),
            never: seed(TypeKind::Never),
            any: seed(TypeKind::Any),
            null: seed(TypeKind::Null),
            undefined: seed(TypeKind::Undefined),
            boolean: seed(TypeKind::Boolean),
        };

        Self { table, intern: RwLock::new(intern), sentinels }
    }

    pub fn unknown(&self) -> TypeId {
        TypeId::from_raw(self.sentinels.unknown)
    }

    pub fn never(&self) -> TypeId {
        TypeId::from_raw(self.sentinels.never)
    }

    pub fn any(&self) -> TypeId {
        TypeId::from_raw(self.sentinels.any)
    }

    pub fn null(&self) -> TypeId {
        TypeId::from_raw(self.sentinels.null)
    }

    pub fn undefined(&self) -> TypeId {
        TypeId::from_raw(self.sentinels.undefined)
    }

    pub fn boolean(&self) -> TypeId {
        TypeId::from_raw(self.sentinels.boolean)
    }
}

impl TypeKernelDb for TypeKernelInner {
    fn intern_type(&self, kind: TypeKind) -> TypeId {
        let canonical = canonicalise(self, kind);

        let guard = self.intern.upgradable_read();
        if let Some(&id) = guard.get(&canonical) {
            return TypeId::from_raw(id);
        }

        let mut guard = RwLockUpgradableReadGuard::upgrade(guard);
        if let Some(&id) = guard.get(&canonical) {
            return TypeId::from_raw(id);
        }

        let raw = self.table.len() as u64;
        self.table.push(Box::new(canonical.clone()));
        guard.insert(canonical, raw);

        TypeId::from_raw(raw)
    }

    fn lookup_type(&self, id: TypeId) -> &TypeKind {
        self.table
            .get(id.raw() as usize)
            .expect("TypeId out of range: caller mixed IDs across db instances or fabricated a TypeId directly")
    }
}

impl TypeKernelDb for RootDatabaseImpl {
    fn intern_type(&self, kind: TypeKind) -> TypeId {
        self.type_kernel_inner().intern_type(kind)
    }

    fn lookup_type(&self, id: TypeId) -> &TypeKind {
        self.type_kernel_inner().lookup_type(id)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use bsl_types::facet::NumberFacet;

    use super::*;

    #[test]
    fn intern_and_lookup_round_trip_via_root_database() {
        let db = RootDatabaseImpl::new();
        let kind = TypeKind::Number(NumberFacet::with_scale(15, 2));

        let id = db.intern_type(kind.clone());

        assert_eq!(db.lookup_type(id), &kind);
    }

    #[test]
    fn cloned_root_database_shares_type_kernel() {
        let db = RootDatabaseImpl::new();
        let clone = db.clone();
        let kind = TypeKind::Number(NumberFacet::with_scale(15, 2));

        let id_from_clone = clone.intern_type(kind.clone());
        let id_from_original = db.intern_type(kind);

        assert!(Arc::ptr_eq(db.type_kernel_inner(), clone.type_kernel_inner()));
        assert_eq!(id_from_clone, id_from_original);
        assert_eq!(db.lookup_type(id_from_clone), clone.lookup_type(id_from_clone));
    }

    #[test]
    fn concurrent_intern_of_same_type_returns_identical_type_id() {
        for _ in 0..1000 {
            let db = RootDatabaseImpl::new();
            let kind = TypeKind::Number(NumberFacet::with_scale(15, 2));

            let left_db = db.clone();
            let left_kind = kind.clone();
            let left = std::thread::spawn(move || left_db.intern_type(left_kind));

            let right_db = db.clone();
            let right = std::thread::spawn(move || right_db.intern_type(kind));

            let left = left.join().expect("left intern thread panicked");
            let right = right.join().expect("right intern thread panicked");

            assert_eq!(left, right);
        }
    }
}
