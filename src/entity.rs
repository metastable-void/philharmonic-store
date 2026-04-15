use crate::error::StoreError;
use crate::revision::{RevisionInput, RevisionRef, RevisionRow};

use philharmonic_types::{Entity, EntityId, Identity, ScalarValue, UnixMillis, Uuid};

use async_trait::async_trait;

/// An entity as read from the entity registry, independent of its revisions.
///
/// Returned by [`EntityStore::get_entity`]. Carries the identity pair, the
/// kind UUID (matching the entity kind's `Entity::KIND` constant), and the
/// creation timestamp. Does not include the entity's revision history;
/// callers that need revisions use [`EntityStore::get_latest_revision`] or
/// [`EntityStore::get_revision`].
#[derive(Clone, Debug)]
pub struct EntityRow {
    /// The entity's identity pair.
    pub identity: Identity,

    /// The entity's kind, matching `T::KIND` for some `T: Entity`.
    pub kind: Uuid,

    /// When the entity was created (when its identity was registered
    /// via `create_entity`).
    pub created_at: UnixMillis,
}

/// Append-only storage for entities and their revision logs.
///
/// An `EntityStore` manages three concerns:
///
/// * Entity registration: associating an identity pair with a kind UUID,
///   creating the row that revisions will hang off of.
/// * Revision append: adding a new immutable revision to an entity's
///   log, carrying content-hash, entity-reference, and scalar attributes.
/// * Revision query: reading back revisions by sequence, finding the
///   latest revision, finding revisions that reference other entities,
///   and finding entities by scalar-attribute values.
///
/// The store is append-only: entities and revisions are never modified or
/// deleted, only added. Soft-delete semantics (tombstone revisions, status
/// flags) live in entity kinds' own schemas, not in the substrate.
///
/// # Concurrency
///
/// Revision sequences are strictly monotonic per entity. When two writers
/// race to append revision `N+1`, the primary-key constraint on
/// `(entity_id, revision_seq)` makes exactly one succeed; the other gets
/// [`StoreError::RevisionConflict`] and should re-read the latest
/// revision and retry with an incremented sequence.
///
/// # Object safety
///
/// This trait is object-safe. For typed operations, see [`EntityStoreExt`].
#[async_trait]
pub trait EntityStore: Send + Sync {
    /// Register an entity with the given identity pair and kind.
    ///
    /// The identity must have been minted previously via
    /// [`IdentityStore::mint`](crate::IdentityStore::mint); this method
    /// does not mint. The entity has no revisions initially; callers
    /// typically follow `create_entity` with one or more
    /// [`append_revision`](Self::append_revision) calls.
    ///
    /// Returns [`StoreError::IdentityCollision`] if an entity with this
    /// identity's internal ID already exists (astronomically unlikely
    /// for freshly-minted identities, but possible if an identity is
    /// used twice by mistake).
    async fn create_entity(
        &self,
        identity: Identity,
        kind: Uuid,
    ) -> Result<(), StoreError>;

    /// Retrieve the entity row for the given internal ID.
    ///
    /// Returns `None` if no entity with this ID has been created. The
    /// returned row does not include revisions; those are fetched
    /// separately.
    async fn get_entity(&self, entity_id: Uuid) -> Result<Option<EntityRow>, StoreError>;

    /// Append a new revision to an entity.
    ///
    /// The caller supplies the expected `revision_seq`, which must be
    /// exactly one greater than the current latest revision's sequence
    /// (or zero if the entity has no revisions yet — implementations
    /// may use 0-based or 1-based sequencing; this method defers to
    /// the caller's choice via the supplied value).
    ///
    /// Writes a row to the revision log plus rows to the attribute
    /// tables for each entry in `input`, atomically (implementations
    /// use a transaction internally).
    ///
    /// Returns [`StoreError::EntityNotFound`] if the entity does not
    /// exist. Returns [`StoreError::RevisionConflict`] if another writer
    /// has already appended a revision at this sequence (caller should
    /// re-read and retry with an incremented sequence).
    async fn append_revision(
        &self,
        entity_id: Uuid,
        revision_seq: u64,
        input: &RevisionInput,
    ) -> Result<(), StoreError>;

    /// Retrieve a specific revision of an entity by sequence.
    ///
    /// Returns `None` if the entity doesn't exist or the revision doesn't
    /// exist at that sequence.
    async fn get_revision(
        &self,
        entity_id: Uuid,
        revision_seq: u64,
    ) -> Result<Option<RevisionRow>, StoreError>;

    /// Retrieve the latest revision of an entity.
    ///
    /// Returns `None` if the entity doesn't exist or has no revisions
    /// yet. "Latest" means highest `revision_seq`; since sequences are
    /// monotonic per entity, this is unambiguous.
    async fn get_latest_revision(
        &self,
        entity_id: Uuid,
    ) -> Result<Option<RevisionRow>, StoreError>;

    /// List revisions that reference the given entity via the given
    /// attribute name.
    ///
    /// Used for reverse-lookup queries: "which templates reference this
    /// config," "which instances reference this template revision," etc.
    /// The `attribute_name` narrows the search to references through a
    /// specific named slot; without it, the query would be too broad
    /// to be useful.
    ///
    /// Returns a vector of `(entity_id, revision_seq)` pairs. The order
    /// is not specified (backends may order by any field for query
    /// performance).
    async fn list_revisions_referencing(
        &self,
        target_entity_id: Uuid,
        attribute_name: &str,
    ) -> Result<Vec<RevisionRef>, StoreError>;

    /// Find entities of a given kind whose latest revision has a scalar
    /// attribute matching the given value.
    ///
    /// Used for indexed-scalar queries: "find active templates"
    /// (`is_deleted=false`), "find failed steps" (`outcome=1`), etc.
    /// The scalar attribute must be declared as `indexed` on the entity
    /// kind's [`ScalarSlot`](philharmonic_types::ScalarSlot) declaration
    /// for this query to be efficient; non-indexed scalars can still
    /// be queried but require table scans.
    ///
    /// Only queries the latest revision of each entity. For historical
    /// queries ("entities whose revision N had this value"), callers
    /// need a different API that doesn't yet exist.
    async fn find_by_scalar(
        &self,
        kind: Uuid,
        attribute_name: &str,
        value: &ScalarValue,
    ) -> Result<Vec<EntityRow>, StoreError>;
}

/// Typed ergonomics on top of [`EntityStore`].
///
/// Provides typed variants of the read methods that verify the entity's
/// kind against the type parameter `T: Entity` and return typed
/// [`EntityId<T>`] values where the raw trait returns [`Identity`] or
/// bare [`Uuid`].
///
/// The read methods return [`StoreError::KindMismatch`] if an entity
/// retrieved from storage has a kind that doesn't match `T::KIND`. This
/// catches data corruption and call-site bugs at the substrate boundary.
///
/// Blanket-implemented for any `EntityStore`.
#[async_trait]
pub trait EntityStoreExt: EntityStore {
    /// Create an entity with the given typed identity.
    ///
    /// The `T` parameter determines the kind UUID written to storage:
    /// the entity row will have `kind = T::KIND`. Equivalent to calling
    /// [`create_entity`](EntityStore::create_entity) with the untyped
    /// identity and `T::KIND`, but expressed in one call.
    async fn create_entity_typed<T: Entity>(
        &self,
        id: EntityId<T>,
    ) -> Result<(), StoreError> {
        self.create_entity(id.untyped(), T::KIND).await
    }

    /// Retrieve an entity row and verify its kind matches `T`.
    ///
    /// Returns `None` if no entity with this ID exists. Returns
    /// [`StoreError::KindMismatch`] if the entity exists but has a
    /// different kind than `T::KIND`.
    async fn get_entity_typed<T: Entity>(
        &self,
        id: EntityId<T>,
    ) -> Result<Option<EntityRow>, StoreError> {
        let Some(row) = self.get_entity(id.internal().as_uuid()).await? else {
            return Ok(None);
        };
        if row.kind != T::KIND {
            return Err(StoreError::KindMismatch {
                expected: T::KIND,
                actual: row.kind,
            });
        }
        Ok(Some(row))
    }

    /// Append a revision to an entity, verifying the entity's kind
    /// matches `T` before writing.
    ///
    /// The kind check is a defense-in-depth against appending a revision
    /// with the wrong shape to an entity of a different kind (the shapes
    /// are declared per-kind on `T`, and appending a template-shaped
    /// revision to an instance-shaped entity would produce garbage).
    ///
    /// Returns the same errors as
    /// [`EntityStore::append_revision`](EntityStore::append_revision),
    /// plus [`StoreError::KindMismatch`] if the entity's kind doesn't
    /// match `T`.
    async fn append_revision_typed<T: Entity>(
        &self,
        id: EntityId<T>,
        revision_seq: u64,
        input: &RevisionInput,
    ) -> Result<(), StoreError> {
        // Verify the entity kind matches before writing.
        let _ = self.get_entity_typed::<T>(id).await?
            .ok_or_else(|| StoreError::EntityNotFound {
                entity_id: id.internal().as_uuid(),
            })?;
        self.append_revision(id.internal().as_uuid(), revision_seq, input).await
    }

    /// Retrieve a revision and verify the parent entity's kind matches `T`.
    ///
    /// The kind verification requires a separate query against the entity
    /// table, so this method does two round trips: one to fetch the
    /// revision, one to verify the entity's kind. Callers that have
    /// already verified the entity's kind (or don't need verification)
    /// should use the untyped [`EntityStore::get_revision`] instead.
    async fn get_revision_typed<T: Entity>(
        &self,
        id: EntityId<T>,
        revision_seq: u64,
    ) -> Result<Option<RevisionRow>, StoreError> {
        let _ = self.get_entity_typed::<T>(id).await?;
        self.get_revision(id.internal().as_uuid(), revision_seq).await
    }

    /// Retrieve the latest revision of an entity and verify its kind
    /// matches `T`.
    ///
    /// Same two-round-trip pattern as
    /// [`get_revision_typed`](Self::get_revision_typed).
    async fn get_latest_revision_typed<T: Entity>(
        &self,
        id: EntityId<T>,
    ) -> Result<Option<RevisionRow>, StoreError> {
        let _ = self.get_entity_typed::<T>(id).await?;
        self.get_latest_revision(id.internal().as_uuid()).await
    }

    /// Find entities of kind `T` whose latest revision has the given
    /// scalar-attribute value.
    ///
    /// Equivalent to [`EntityStore::find_by_scalar`](EntityStore::find_by_scalar)
    /// with `T::KIND` as the kind argument, but expressed in one call.
    async fn find_by_scalar_typed<T: Entity>(
        &self,
        attribute_name: &str,
        value: &ScalarValue,
    ) -> Result<Vec<EntityRow>, StoreError> {
        self.find_by_scalar(T::KIND, attribute_name, value).await
    }
}

impl<S: EntityStore + ?Sized> EntityStoreExt for S {}
