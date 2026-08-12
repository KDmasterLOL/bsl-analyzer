//! Where knowledge that a file was indexed actually lives.
//!
//! A key's knowledge is not held in one place. The store row is a snapshot of the
//! boot walk and stops growing the moment the daemon is up; everything indexed
//! afterwards lives in the overlay alone; a file whose contents could not be read
//! leaves only an obligation to re-read it; the fingerprint row outlives its
//! entry; and against a remote baseline the manifest is the only carrier there is,
//! because the local rows are cleared on boot.
//!
//! An operation that asks one carrier therefore asks the wrong question. This
//! module is the single place that enumerates them, so "does anything still know
//! about this key" has one answer and adding a carrier is a compile error rather
//! than a silently narrower reconcile.

use std::collections::HashSet;

use crate::workspace_roots::FileKey;

/// A carrier of POSITIVE knowledge: evidence that the key was indexed and is
/// expected to exist.
///
/// Tombstones and context marks are deliberately absent. A tombstone records a
/// file's ABSENCE, so it can never make a key a candidate for removal, and a
/// context mark says a rendered context went stale — a claim about freshness,
/// not about existence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum KeyCarrier {
    /// The `files` row of the `code` collection, with its cascaded chunks.
    StoreRow,
    /// The overlay entry serving the file's current contents.
    OverlayEntry,
    /// The standing obligation to re-read a file proven present but unreadable.
    UnreadObligation,
    /// The persisted fingerprint row asserting the file was verified.
    FingerprintRow,
    /// The published baseline manifest, consulted only where it is served.
    BaselineManifest,
}

impl KeyCarrier {
    /// Every carrier, in a fixed order. The array length is part of the type, so
    /// a new variant fails to compile here until it is listed.
    pub(crate) const ALL: [KeyCarrier; 5] = [
        KeyCarrier::StoreRow,
        KeyCarrier::OverlayEntry,
        KeyCarrier::UnreadObligation,
        KeyCarrier::FingerprintRow,
        KeyCarrier::BaselineManifest,
    ];
}

/// One reading of every carrier, taken once per operation rather than once per
/// key: each carrier costs a load, and a per-key lookup would turn a reconcile
/// into one query per stored file.
///
/// A carrier the caller cannot read — the manifest outside the mode that serves
/// it, the overlay behind a poisoned lock — is left empty rather than guessed at.
#[derive(Debug, Default)]
pub(crate) struct CarrierKeys {
    pub(crate) store_rows: HashSet<FileKey>,
    pub(crate) overlay_entries: HashSet<FileKey>,
    pub(crate) unread: HashSet<FileKey>,
    pub(crate) fingerprints: HashSet<FileKey>,
    pub(crate) manifest: HashSet<FileKey>,
}

impl CarrierKeys {
    /// The keys held by one carrier. The total `match` is what makes the
    /// enumeration honest: a carrier added to [`KeyCarrier`] and forgotten here
    /// does not compile.
    fn keys_of(&self, carrier: KeyCarrier) -> &HashSet<FileKey> {
        match carrier {
            KeyCarrier::StoreRow => &self.store_rows,
            KeyCarrier::OverlayEntry => &self.overlay_entries,
            KeyCarrier::UnreadObligation => &self.unread,
            KeyCarrier::FingerprintRow => &self.fingerprints,
            KeyCarrier::BaselineManifest => &self.manifest,
        }
    }

    /// Which carriers still know about `key`, in [`KeyCarrier::ALL`] order.
    pub(crate) fn carriers_of(&self, key: &FileKey) -> Vec<KeyCarrier> {
        KeyCarrier::ALL.into_iter().filter(|carrier| self.keys_of(*carrier).contains(key)).collect()
    }

    /// Every key any carrier knows about — the candidates a reconcile has to
    /// consider, as opposed to the store rows it used to walk.
    pub(crate) fn all_keys(&self) -> HashSet<FileKey> {
        let mut keys = HashSet::new();
        for carrier in KeyCarrier::ALL {
            keys.extend(self.keys_of(carrier).iter().cloned());
        }
        keys
    }

    /// Whether the manifest is the only carrier holding `key`.
    ///
    /// The manifest is a snapshot of someone else's corpus, so removing a key
    /// does not delete its row — the removal is expressed by hiding it instead.
    /// Such a key would otherwise be re-selected by every later reconcile,
    /// growing the removal count without a single change to what search serves.
    pub(crate) fn manifest_is_sole_carrier(&self, key: &FileKey) -> bool {
        self.carriers_of(key) == [KeyCarrier::BaselineManifest]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(path: &str) -> FileKey {
        FileKey::configuration(path)
    }

    /// Each carrier answers for itself: a key living in exactly one of them is
    /// still known. Checked per carrier rather than in bulk — a reading that
    /// looked at only one set would satisfy any single-carrier assertion.
    #[test]
    fn one_carrier_alone_is_enough_to_know_a_key() {
        let subject = key("A.bsl");
        for carrier in KeyCarrier::ALL {
            let mut carriers = CarrierKeys::default();
            match carrier {
                KeyCarrier::StoreRow => carriers.store_rows.insert(subject.clone()),
                KeyCarrier::OverlayEntry => carriers.overlay_entries.insert(subject.clone()),
                KeyCarrier::UnreadObligation => carriers.unread.insert(subject.clone()),
                KeyCarrier::FingerprintRow => carriers.fingerprints.insert(subject.clone()),
                KeyCarrier::BaselineManifest => carriers.manifest.insert(subject.clone()),
            };
            assert!(
                !carriers.carriers_of(&subject).is_empty(),
                "{carrier:?} did not answer for its own key",
            );
            assert_eq!(carriers.carriers_of(&subject), vec![carrier]);
            assert_eq!(carriers.all_keys(), HashSet::from([subject.clone()]));
        }
    }

    #[test]
    fn an_unknown_key_is_held_by_nothing() {
        let mut carriers = CarrierKeys::default();
        carriers.store_rows.insert(key("A.bsl"));
        assert!(carriers.carriers_of(&key("B.bsl")).is_empty());
    }

    /// The manifest counts as sole carrier only when it truly is the only one:
    /// a key that also has a local row must stay a candidate, because its
    /// removal empties that row for good.
    #[test]
    fn a_manifest_key_with_a_local_row_is_not_manifest_only() {
        let subject = key("A.bsl");
        let mut carriers = CarrierKeys::default();
        carriers.manifest.insert(subject.clone());
        assert!(carriers.manifest_is_sole_carrier(&subject));
        carriers.store_rows.insert(subject.clone());
        assert!(!carriers.manifest_is_sole_carrier(&subject));
    }
}
