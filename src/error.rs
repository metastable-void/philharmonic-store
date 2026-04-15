use philharmonic_types::{ContentDecodeError, IdKindError, IdentityKindError, Uuid};

/// Errors returned by the `philharmonic-store` substrate.
///
/// Variants partition into three groups:
///
/// * Semantic outcomes (`KindMismatch`, `Decode`, `IdKind`, `IdentityKind`,
///   `ScalarTypeMismatch`) — the data on disk doesn't match what the caller
///   expected. Suggests either a bug at the call site, schema drift, or
///   data produced by an incompatible writer.
/// * Concurrency outcomes (`RevisionConflict`, `IdentityCollision`) — the
///   requested operation lost a race against another writer, or (for
///   `IdentityCollision`) against astronomical UUID collision odds. The
///   caller can retry; for revision conflicts, that means re-reading and
///   incrementing the sequence.
/// * Backend failures (`Backend`) — the storage backend reported an error
///   that doesn't map to a substrate-level semantic. Backend implementations
///   translate their internal errors here when no specific variant applies.
///
/// Not represented as errors:
///
/// * "Row not found" on read paths — substrate read methods return
///   `Option<T>` rather than erroring, because absence is a normal outcome
///   the caller is expected to handle, not an exceptional condition.
///   Write paths that *require* a parent to exist return a specific variant
///   (`EntityNotFound`) because there absence is unexpected.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    /// An entity was retrieved with a kind UUID that didn't match the
    /// caller's expected kind.
    ///
    /// Returned by typed read paths (e.g., `EntityStoreExt::get_entity_typed`)
    /// when the row's stored kind disagrees with the type parameter.
    /// Indicates either a bug at the call site (asking for the wrong type)
    /// or schema drift (the same UUID being reused for different kinds
    /// across deployments — which shouldn't happen given the global-UUID
    /// discipline, but the check is cheap).
    #[error("entity kind mismatch: expected {expected}, found {actual}")]
    KindMismatch { expected: Uuid, actual: Uuid },

    /// Bytes retrieved from the content store could not be decoded as the
    /// expected `Content` type.
    ///
    /// Returned by typed read paths (e.g., `ContentStoreExt::get_typed`)
    /// when `T::from_content_bytes` fails. Indicates that the bytes stored
    /// at this hash are not a valid encoding of `T` — usually because the
    /// caller is asking for the wrong type, occasionally because the
    /// content store contains corrupted data.
    #[error("content decode failed: {0}")]
    Decode(#[from] ContentDecodeError),

    /// A UUID retrieved from storage doesn't have the expected version.
    ///
    /// Surfaces violations of the `InternalId`-is-UUIDv7 /
    /// `PublicId`-is-UUIDv4 invariant. Should never occur in practice
    /// because writes go through validating constructors, but the check
    /// at the read boundary is cheap and catches data corruption.
    #[error("UUID version mismatch in storage: {0}")]
    IdKind(#[from] IdKindError),

    /// An identity pair retrieved from storage had the wrong UUID
    /// versions for its internal/public roles.
    ///
    /// Compound version of `IdKind` for the `Identity` two-UUID case.
    #[error("identity version mismatch in storage: {0}")]
    IdentityKind(#[from] IdentityKindError),

    /// A scalar value's runtime type doesn't match the column shape the
    /// substrate expected for the given attribute.
    ///
    /// Should not occur if writers go through the typed extension API,
    /// which enforces type consistency at compile time. Returned by the
    /// untyped API when, for example, a `ScalarValue::Bool` is provided
    /// for an attribute whose row stores `value_i64`.
    #[error("scalar type mismatch for attribute {attribute_name}: {detail}")]
    ScalarTypeMismatch {
        attribute_name: String,
        detail: String,
    },

    /// An attempt to append a revision lost a race against another writer.
    ///
    /// Occurs when two writers compute the same `next_revision_seq` from
    /// reads that happened before either committed; the second to commit
    /// hits the `(entity_id, revision_seq)` primary-key constraint and
    /// fails. The caller should re-read the latest revision and retry
    /// with an incremented sequence.
    #[error("revision conflict: entity {entity_id}, seq {revision_seq} already exists")]
    RevisionConflict { entity_id: Uuid, revision_seq: u64 },

    /// An attempt to create an entity collided with an existing identity.
    ///
    /// Vanishingly rare with UUIDv7+UUIDv4 generation, but the check is
    /// free given the unique constraints on the identity table. The caller
    /// should mint a fresh identity and retry.
    #[error("identity collision: {uuid}")]
    IdentityCollision { uuid: Uuid },

    /// A write operation referenced an entity that doesn't exist.
    ///
    /// Distinct from `Option::None` on a read: this is for *writes* that
    /// require a parent to exist. For example, `append_revision(entity_id, ...)`
    /// returns this if no entity with `entity_id` has been created.
    #[error("entity not found: {entity_id}")]
    EntityNotFound { entity_id: Uuid },

    /// The storage backend reported an error that doesn't map to a
    /// substrate-level semantic.
    ///
    /// Backend implementations (e.g., `philharmonic-store-sqlx-mysql`)
    /// translate their internal failures to this variant when no specific
    /// substrate variant applies. The carried `BackendError` includes a
    /// human-readable message and a hint about whether retry is sensible.
    #[error("{0}")]
    Backend(#[from] BackendError),
}

impl StoreError {
    /// Whether this error is potentially recoverable by retry.
    ///
    /// Consumers implementing retry logic can use this as a hint:
    ///
    /// * Concurrency outcomes (`RevisionConflict`, `IdentityCollision`)
    ///   are retryable — re-read and try again.
    /// * Backend errors carry their own retryability hint, set by the
    ///   backend's translator.
    /// * Semantic violations are not retryable — they indicate bugs or
    ///   data corruption that retrying won't fix.
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::RevisionConflict { .. } | Self::IdentityCollision { .. } => true,
            Self::Backend(e) => e.retryable,
            Self::KindMismatch { .. }
            | Self::Decode(_)
            | Self::IdKind(_)
            | Self::IdentityKind(_)
            | Self::ScalarTypeMismatch { .. }
            | Self::EntityNotFound { .. } => false,
        }
    }
}

/// A backend-specific failure, carried inside `StoreError::Backend`.
///
/// Backend implementations translate their native error types into this
/// shape when reporting failures that don't have a substrate-level meaning
/// (database connection lost, deadlock detected, etc.). The substrate
/// itself doesn't introspect backend errors; consumers either log them or
/// retry based on the `retryable` hint.
///
/// The `message` is intended for human consumption (logs, error reports);
/// it is not stable and should not be parsed. The `retryable` flag is set
/// by the backend's classifier and reflects backend-specific knowledge
/// (e.g., MySQL deadlock errors are retryable, schema mismatches are not).
#[derive(Debug, thiserror::Error)]
#[error("backend error: {message}")]
pub struct BackendError {
    pub message: String,
    pub retryable: bool,
}

impl BackendError {
    /// Construct a backend error with a message and retryability hint.
    pub fn new(message: impl Into<String>, retryable: bool) -> Self {
        Self {
            message: message.into(),
            retryable,
        }
    }

    /// Convenience constructor for a non-retryable backend error.
    pub fn fatal(message: impl Into<String>) -> Self {
        Self::new(message, false)
    }

    /// Convenience constructor for a retryable backend error.
    pub fn transient(message: impl Into<String>) -> Self {
        Self::new(message, true)
    }
}
