use crate::error::StoreError;

use philharmonic_types::{Entity, EntityId, Identity, Uuid};

use async_trait::async_trait;

/// Identity-pair minting and resolution.
///
/// An `IdentityStore` manages the registry of `(internal, public)` UUID
/// pairs used to address entities across the system. Internal IDs are
/// UUIDv7 (time-ordered, used for internal storage and indexing); public
/// IDs are UUIDv4 (opaque, used for external references that should not
/// leak creation ordering).
///
/// The store is append-only: once minted, an identity pair exists forever.
/// Entities that reference an identity may come and go (via soft-delete
/// tombstones on their own revisions), but the identity row itself is
/// never removed. This simplifies concurrency (no coordination needed for
/// read-after-write) and matches the substrate's grow-only discipline.
///
/// # Relationship to `EntityStore`
///
/// Minting an identity does not create an entity — it just reserves the
/// UUID pair. To materialize an entity that can hold revisions, pair a
/// `mint` call with [`EntityStore::create_entity`](crate::EntityStore::create_entity).
/// The convenience method [`StoreExt::create_entity_minting`](crate::StoreExt::create_entity_minting)
/// does both atomically from the caller's perspective.
///
/// # Orphan identities
///
/// If a caller mints an identity and then fails to create the
/// corresponding entity (due to a crash, an error in intervening logic,
/// or simply changing their mind), the identity row remains as an orphan.
/// This is harmless — 32 bytes of unreferenced UUIDs. Deployments that
/// care can run a periodic GC job that deletes identity rows not
/// referenced by any entity table; the substrate does not provide one.
///
/// # Object safety
///
/// This trait is object-safe. For typed operations, see [`IdentityStoreExt`].
#[async_trait]
pub trait IdentityStore: Send + Sync {
    /// Mint a fresh identity pair.
    ///
    /// Generates a new UUIDv7 for the internal ID and a new UUIDv4 for
    /// the public ID, inserts the pair into the identity registry, and
    /// returns it.
    ///
    /// Returns [`StoreError::IdentityCollision`] in the astronomically
    /// unlikely event that the generated UUID already exists. Callers
    /// should retry on that outcome (a fresh mint will produce different
    /// UUIDs).
    async fn mint(&self) -> Result<Identity, StoreError>;

    /// Resolve a public ID to its full identity pair.
    ///
    /// Returns `None` if no identity with this public ID exists. Used
    /// primarily at API boundaries, where inbound requests reference
    /// entities by their opaque public IDs and the substrate needs the
    /// internal ID for subsequent queries.
    async fn resolve_public(&self, public: Uuid) -> Result<Option<Identity>, StoreError>;

    /// Resolve an internal ID to its full identity pair.
    ///
    /// Returns `None` if no identity with this internal ID exists. Used
    /// when the substrate has an internal ID in hand (from a foreign-key
    /// reference, say) and needs the corresponding public ID to render
    /// it over an external API.
    async fn resolve_internal(&self, internal: Uuid) -> Result<Option<Identity>, StoreError>;
}

/// Typed ergonomics on top of [`IdentityStore`].
///
/// The base trait deals in raw [`Identity`] pairs, whose UUIDs carry no
/// compile-time indication of what kind of entity they identify. This
/// extension trait provides methods that return and accept [`EntityId<T>`],
/// which carries the entity kind in its phantom type parameter.
///
/// Typed methods validate UUID versions at the substrate boundary: an
/// identity retrieved from storage must have a UUIDv7 internal and
/// UUIDv4 public, or the operation returns [`StoreError::IdentityKind`].
/// This catches corruption that would otherwise manifest as subtle
/// downstream bugs.
///
/// Blanket-implemented for any `IdentityStore`: consumers that import
/// `IdentityStoreExt` alongside `IdentityStore` get the typed methods
/// automatically.
#[async_trait]
pub trait IdentityStoreExt: IdentityStore {
    /// Mint a fresh identity pair typed as `EntityId<T>`.
    ///
    /// Equivalent to [`mint`](IdentityStore::mint) followed by
    /// [`Identity::typed`], but fused into one call for ergonomics.
    /// The `T` parameter is compile-time only; the substrate stores the
    /// pair untyped and the type is recovered at the read boundary.
    async fn mint_typed<T: Entity>(&self) -> Result<EntityId<T>, StoreError> {
        let identity = self.mint().await?;
        identity.typed::<T>().map_err(StoreError::from)
    }

    /// Resolve a typed public ID to a typed `EntityId<T>`.
    ///
    /// Takes a bare [`Uuid`] rather than a typed `PublicId<T>` because
    /// public IDs come in over external boundaries as raw UUIDs — there's
    /// no opportunity for the caller to have already typed them. The
    /// returned `EntityId<T>` is typed because the caller is asserting
    /// (via the type parameter) what kind of entity they expect.
    ///
    /// Note that the type parameter `T` is not verified against the
    /// identity row itself — the identity table doesn't store kind
    /// information. Kind verification happens at the entity-store layer
    /// via [`EntityStoreExt`](crate::EntityStoreExt).
    async fn resolve_public_typed<T: Entity>(
        &self,
        public: Uuid,
    ) -> Result<Option<EntityId<T>>, StoreError> {
        let Some(identity) = self.resolve_public(public).await? else {
            return Ok(None);
        };
        Ok(Some(identity.typed::<T>()?))
    }

    /// Resolve a typed internal ID to a typed `EntityId<T>`.
    ///
    /// Same caveat as [`resolve_public_typed`](Self::resolve_public_typed):
    /// kind is not verified by the identity store alone.
    async fn resolve_internal_typed<T: Entity>(
        &self,
        internal: Uuid,
    ) -> Result<Option<EntityId<T>>, StoreError> {
        let Some(identity) = self.resolve_internal(internal).await? else {
            return Ok(None);
        };
        Ok(Some(identity.typed::<T>()?))
    }
}

impl<S: IdentityStore + ?Sized> IdentityStoreExt for S {}
