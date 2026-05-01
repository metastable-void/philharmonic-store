use philharmonic_types::{ScalarValue, Sha256, UnixMillis, Uuid};

use std::collections::HashMap;

/// Input to an entity revision append.
///
/// Contains the three kinds of attributes an entity revision can have:
/// content references (hashes into the content store), entity references
/// (pointers to other entities, optionally pinned to specific revisions),
/// and scalar values (small typed fields stored directly on the revision
/// for queryability).
///
/// Attribute names are scoped to the entity kind: a `"metadata"` content
/// slot on one entity kind is unrelated to a `"metadata"` content slot on
/// another. The substrate enforces this implicitly via the primary key
/// `(entity_id, revision_seq, attribute_name)` on each attribute table —
/// names only collide within the same revision.
///
/// `RevisionInput` is constructed by callers building up a new revision
/// and passed to [`EntityStore::append_revision`](crate::EntityStore::append_revision).
/// Typed consumers going through [`EntityStoreExt`](crate::EntityStoreExt)
/// normally don't construct this directly — the typed API builds it from
/// an `Entity` declaration and a struct of field values.
#[derive(Clone, Debug, Default)]
pub struct RevisionInput {
    /// Content-hash attributes: attribute name → hash of content bytes.
    ///
    /// Each entry corresponds to a `ContentSlot` declaration on the
    /// entity kind. The hash must refer to content already present in
    /// the content store; the substrate does not upload content as a
    /// side effect of appending a revision.
    pub content_attrs: HashMap<String, Sha256>,

    /// Entity-reference attributes: attribute name → reference to
    /// another entity.
    ///
    /// Each entry corresponds to an `EntitySlot` declaration on the
    /// entity kind. The referenced entity must exist; the substrate
    /// does not create entities as a side effect of references.
    pub entity_attrs: HashMap<String, EntityRefValue>,

    /// Scalar attributes: attribute name → typed scalar value.
    ///
    /// Each entry corresponds to a `ScalarSlot` declaration on the
    /// entity kind, with `ScalarValue` type matching the slot's
    /// `ScalarType`. Mismatched types return
    /// [`StoreError::ScalarTypeMismatch`](crate::StoreError::ScalarTypeMismatch)
    /// at the substrate layer.
    pub scalar_attrs: HashMap<String, ScalarValue>,
}

impl RevisionInput {
    /// Construct an empty revision input.
    ///
    /// Equivalent to `Default::default()`. Useful when building up a
    /// revision incrementally with `.with_content`, `.with_entity`,
    /// `.with_scalar`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set a content-hash attribute, returning `self` for chaining.
    pub fn with_content(mut self, name: impl Into<String>, hash: Sha256) -> Self {
        self.content_attrs.insert(name.into(), hash);
        self
    }

    /// Set an entity-reference attribute, returning `self` for chaining.
    pub fn with_entity(mut self, name: impl Into<String>, reference: EntityRefValue) -> Self {
        self.entity_attrs.insert(name.into(), reference);
        self
    }

    /// Set a scalar attribute, returning `self` for chaining.
    pub fn with_scalar(mut self, name: impl Into<String>, value: ScalarValue) -> Self {
        self.scalar_attrs.insert(name.into(), value);
        self
    }
}

/// A reference from one entity revision to another entity, optionally
/// pinned to a specific revision of the target.
///
/// Corresponds to an `EntitySlot` declaration on the source entity kind.
/// The `pinning` choice is recorded per-reference: different attributes
/// on the same revision may reference the same target with different
/// pinning semantics (e.g., one attribute pins to a specific config
/// revision while another tracks the latest).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EntityRefValue {
    /// Internal ID of the target entity.
    pub target_entity_id: Uuid,

    /// Revision of the target to reference.
    ///
    /// * `Some(seq)`: pinned to revision `seq`. Reads always return that
    ///   specific revision regardless of newer revisions that may exist.
    /// * `None`: tracks the latest revision. Reads return whatever
    ///   revision is current at read time, which may change over time.
    pub target_revision_seq: Option<u64>,
}

impl EntityRefValue {
    /// Construct a reference that tracks the target's latest revision.
    pub fn latest(target: Uuid) -> Self {
        Self {
            target_entity_id: target,
            target_revision_seq: None,
        }
    }

    /// Construct a reference pinned to a specific revision of the target.
    pub fn pinned(target: Uuid, revision_seq: u64) -> Self {
        Self {
            target_entity_id: target,
            target_revision_seq: Some(revision_seq),
        }
    }

    /// Whether this reference is pinned to a specific revision.
    pub fn is_pinned(&self) -> bool {
        self.target_revision_seq.is_some()
    }
}

/// An entity revision as read from storage.
///
/// The read counterpart to [`RevisionInput`]: same three attribute kinds
/// plus the revision's metadata (its entity, sequence number, and
/// creation time). Returned by [`EntityStore::get_revision`](crate::EntityStore::get_revision)
/// and related read methods.
#[derive(Clone, Debug)]
pub struct RevisionRow {
    /// Internal ID of the entity this revision belongs to.
    pub entity_id: Uuid,

    /// Sequence number of this revision within the entity.
    ///
    /// Revisions are numbered starting from 0 or 1 (implementation
    /// choice; the substrate doesn't prescribe). Sequences are strictly
    /// monotonically increasing per entity.
    pub revision_seq: u64,

    /// When this revision was appended.
    pub created_at: UnixMillis,

    /// Content-hash attributes on this revision.
    pub content_attrs: HashMap<String, Sha256>,

    /// Entity-reference attributes on this revision.
    pub entity_attrs: HashMap<String, EntityRefValue>,

    /// Scalar attributes on this revision.
    pub scalar_attrs: HashMap<String, ScalarValue>,
}

/// A lightweight reference to an entity revision, without its attributes.
///
/// Returned by query methods like
/// [`EntityStore::list_revisions_referencing`](crate::EntityStore::list_revisions_referencing),
/// where the caller needs to identify revisions but typically doesn't
/// need their full contents immediately. Callers that need the full
/// revision can follow up with
/// [`EntityStore::get_revision`](crate::EntityStore::get_revision) using
/// the two fields here.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RevisionRef {
    /// Internal ID of the entity.
    pub entity_id: Uuid,
    /// Sequence number of the revision.
    pub revision_seq: u64,
}

impl RevisionRef {
    /// Create a reference to a specific entity revision.
    pub fn new(entity_id: Uuid, revision_seq: u64) -> Self {
        Self {
            entity_id,
            revision_seq,
        }
    }
}
