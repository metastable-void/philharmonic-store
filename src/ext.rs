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

#[cfg(test)]
mod tests {
    use crate::entity::{EntityRow, EntityStore};
    use crate::error::{BackendError, StoreError};
    use crate::ext::StoreExt;
    use crate::identity::IdentityStore;
    use crate::revision::{RevisionInput, RevisionRef, RevisionRow};

    use philharmonic_types::{
        ContentSlot, Entity, EntityId, EntitySlot, Identity, ScalarSlot, ScalarValue, UnixMillis,
        Uuid,
    };

    use std::collections::HashMap;
    use std::sync::Mutex;

    use async_trait::async_trait;

    struct TestEntity;

    impl Entity for TestEntity {
        const KIND: Uuid = Uuid::from_u128(0xC0FFEE);
        const NAME: &'static str = "test_entity";
        const CONTENT_SLOTS: &'static [ContentSlot] = &[];
        const ENTITY_SLOTS: &'static [EntitySlot] = &[];
        const SCALAR_SLOTS: &'static [ScalarSlot] = &[];
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum CombinedCall {
        Mint,
        CreateEntity { identity: Identity, kind: Uuid },
    }

    struct CombinedStore {
        by_internal: Mutex<HashMap<Uuid, Uuid>>,
        by_public: Mutex<HashMap<Uuid, Uuid>>,
        entities: Mutex<HashMap<Uuid, EntityRow>>,
        calls: Mutex<Vec<CombinedCall>>,
        next_mint: Mutex<Option<Identity>>,
        fail_create: Mutex<Option<String>>,
    }

    impl CombinedStore {
        fn new() -> Self {
            Self {
                by_internal: Mutex::new(HashMap::new()),
                by_public: Mutex::new(HashMap::new()),
                entities: Mutex::new(HashMap::new()),
                calls: Mutex::new(Vec::new()),
                next_mint: Mutex::new(None),
                fail_create: Mutex::new(None),
            }
        }

        fn set_next_mint(&self, identity: Identity) {
            *self.next_mint.lock().unwrap() = Some(identity);
        }

        fn fail_create_with(&self, message: impl Into<String>) {
            *self.fail_create.lock().unwrap() = Some(message.into());
        }

        fn calls(&self) -> Vec<CombinedCall> {
            self.calls.lock().unwrap().clone()
        }

        fn has_identity(&self, internal: Uuid) -> bool {
            self.by_internal.lock().unwrap().contains_key(&internal)
        }

        fn has_entity(&self, internal: Uuid) -> bool {
            self.entities.lock().unwrap().contains_key(&internal)
        }
    }

    #[async_trait]
    impl IdentityStore for CombinedStore {
        async fn mint(&self) -> Result<Identity, StoreError> {
            self.calls.lock().unwrap().push(CombinedCall::Mint);
            let identity = self.next_mint.lock().unwrap().take().unwrap_or(Identity {
                internal: Uuid::now_v7(),
                public: Uuid::new_v4(),
            });
            self.by_internal
                .lock()
                .unwrap()
                .insert(identity.internal, identity.public);
            self.by_public
                .lock()
                .unwrap()
                .insert(identity.public, identity.internal);
            Ok(identity)
        }

        async fn resolve_public(&self, public: Uuid) -> Result<Option<Identity>, StoreError> {
            let Some(internal) = self.by_public.lock().unwrap().get(&public).copied() else {
                return Ok(None);
            };
            Ok(Some(Identity { internal, public }))
        }

        async fn resolve_internal(&self, internal: Uuid) -> Result<Option<Identity>, StoreError> {
            let Some(public) = self.by_internal.lock().unwrap().get(&internal).copied() else {
                return Ok(None);
            };
            Ok(Some(Identity { internal, public }))
        }
    }

    #[async_trait]
    impl EntityStore for CombinedStore {
        async fn create_entity(&self, identity: Identity, kind: Uuid) -> Result<(), StoreError> {
            self.calls
                .lock()
                .unwrap()
                .push(CombinedCall::CreateEntity { identity, kind });
            if let Some(message) = self.fail_create.lock().unwrap().clone() {
                return Err(StoreError::Backend(BackendError::fatal(message)));
            }
            self.entities.lock().unwrap().insert(
                identity.internal,
                EntityRow {
                    identity,
                    kind,
                    created_at: UnixMillis(5),
                },
            );
            Ok(())
        }

        async fn get_entity(&self, entity_id: Uuid) -> Result<Option<EntityRow>, StoreError> {
            Ok(self.entities.lock().unwrap().get(&entity_id).cloned())
        }

        async fn append_revision(
            &self,
            entity_id: Uuid,
            _revision_seq: u64,
            _input: &RevisionInput,
        ) -> Result<(), StoreError> {
            if !self.entities.lock().unwrap().contains_key(&entity_id) {
                return Err(StoreError::EntityNotFound { entity_id });
            }
            Ok(())
        }

        async fn get_revision(
            &self,
            _entity_id: Uuid,
            _revision_seq: u64,
        ) -> Result<Option<RevisionRow>, StoreError> {
            Ok(None)
        }

        async fn get_latest_revision(
            &self,
            _entity_id: Uuid,
        ) -> Result<Option<RevisionRow>, StoreError> {
            Ok(None)
        }

        async fn list_revisions_referencing(
            &self,
            _target_entity_id: Uuid,
            _attribute_name: &str,
        ) -> Result<Vec<RevisionRef>, StoreError> {
            Ok(Vec::new())
        }

        async fn find_by_scalar(
            &self,
            _kind: Uuid,
            _attribute_name: &str,
            _value: &ScalarValue,
        ) -> Result<Vec<EntityRow>, StoreError> {
            Ok(Vec::new())
        }
    }

    fn seeded_identity() -> Identity {
        Identity {
            internal: Uuid::now_v7(),
            public: Uuid::new_v4(),
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn create_entity_minting_calls_mint_then_create_entity_and_returns_id() {
        let store = CombinedStore::new();
        let expected_identity = seeded_identity();
        store.set_next_mint(expected_identity);

        let id: EntityId<TestEntity> = store.create_entity_minting::<TestEntity>().await.unwrap();

        assert_eq!(id.untyped(), expected_identity);
        assert_eq!(
            store.calls(),
            vec![
                CombinedCall::Mint,
                CombinedCall::CreateEntity {
                    identity: expected_identity,
                    kind: TestEntity::KIND,
                }
            ]
        );
        assert!(store.has_entity(expected_identity.internal));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn create_entity_minting_returns_create_error_and_keeps_orphaned_identity() {
        let store = CombinedStore::new();
        let minted = seeded_identity();
        store.set_next_mint(minted);
        store.fail_create_with("create failed");

        let err = store
            .create_entity_minting::<TestEntity>()
            .await
            .unwrap_err();

        match err {
            StoreError::Backend(backend) => assert_eq!(backend.message, "create failed"),
            other => panic!("expected backend error, got {other:?}"),
        }
        assert_eq!(
            store.calls(),
            vec![
                CombinedCall::Mint,
                CombinedCall::CreateEntity {
                    identity: minted,
                    kind: TestEntity::KIND,
                }
            ]
        );
        assert!(store.has_identity(minted.internal));
        assert!(!store.has_entity(minted.internal));
    }
}
