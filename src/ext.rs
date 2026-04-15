use crate::entity::{EntityStore, EntityStoreExt};
use crate::error::StoreError;
use crate::identity::{IdentityStore, IdentityStoreExt};

use philharmonic_types::{Entity, EntityId};

use async_trait::async_trait;

/// Cross-concern conveniences for the storage substrate.
///
/// [`IdentityStore`], [`EntityStore`], and [`ContentStore`](crate::ContentStore)
/// each handle a single substrate concern with a minimal API. Some common
/// workflows span multiple concerns, and expressing them as one-liners
/// here saves callers from writing the same multi-step pattern repeatedly.
///
/// `StoreExt` is a supertrait bundle — any type implementing both
/// `IdentityStore` and `EntityStore` automatically implements `StoreExt`
/// via a blanket impl. Consumers who want the cross-concern methods
/// import `StoreExt` alongside the base traits.
///
/// # Design note
///
/// Methods on `StoreExt` are genuine cross-concern operations, not mere
/// re-exports of single-concern methods with convenience bindings. The
/// dividing line: if a method could live on `IdentityStoreExt` or
/// `EntityStoreExt` alone, it belongs there; if it requires both traits,
/// it belongs here.
#[async_trait]
pub trait StoreExt: IdentityStore + EntityStore {
    /// Mint a fresh identity and create an entity with it, in one call.
    ///
    /// Equivalent to:
    ///
    /// ```ignore
    /// let id = store.mint_typed::<T>().await?;
    /// store.create_entity_typed::<T>(id).await?;
    /// ```
    ///
    /// Returns the typed [`EntityId<T>`] for the newly-created entity.
    ///
    /// # Failure modes
    ///
    /// If `mint` succeeds and `create_entity` fails, the minted identity
    /// is orphaned — a row in the identity table with no corresponding
    /// entity row. This is harmless (see [`IdentityStore`] docs for
    /// discussion) and does not need explicit cleanup. A subsequent
    /// retry mints a fresh identity; the orphan remains but references
    /// nothing.
    ///
    /// Errors are returned as-is: [`StoreError::IdentityCollision`] if
    /// UUID generation collides (vanishingly rare), or any backend
    /// error from either call.
    async fn create_entity_minting<T: Entity>(&self) -> Result<EntityId<T>, StoreError> {
        let id = self.mint_typed::<T>().await?;
        self.create_entity_typed::<T>(id).await?;
        Ok(id)
    }
}

impl<S: IdentityStore + EntityStore + ?Sized> StoreExt for S {}
