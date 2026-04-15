use crate::error::StoreError;

use philharmonic_types::{Content, ContentHash, ContentValue, Sha256};

use async_trait::async_trait;

/// Content-addressed blob storage.
///
/// A `ContentStore` maps SHA-256 hashes to byte sequences. It is grow-only:
/// writes never modify or delete existing rows, so operations are
/// independently atomic at the backend's single-statement level and the
/// trait requires no transactional API.
///
/// # Idempotency
///
/// [`put`](Self::put) is idempotent: writing the same bytes twice produces
/// the same hash and the same row, with no error on the second write.
/// Backends implement this via `INSERT IGNORE` or equivalent.
///
/// # Consistency
///
/// Implementations must guarantee read-your-own-writes within a single
/// `ContentStore` instance: after `put(value)` returns successfully, a
/// subsequent `get(value.digest())` or `exists(value.digest())` call on
/// the same instance must observe the written row.
///
/// # Object safety
///
/// This trait is object-safe. Consumers holding `&dyn ContentStore` get
/// dynamic dispatch; consumers holding `impl ContentStore` get static
/// dispatch. For typed operations, see [`ContentStoreExt`].
#[async_trait]
pub trait ContentStore: Send + Sync {
    /// Store a content value. Idempotent: storing the same bytes twice
    /// is not an error.
    ///
    /// The value's hash is the storage key; callers who need the hash
    /// for subsequent operations should read it from the value itself
    /// (via [`ContentValue::digest`]) before or after this call.
    async fn put(&self, value: &ContentValue) -> Result<(), StoreError>;

    /// Retrieve a content value by its hash.
    ///
    /// Returns `None` if no blob with this hash exists in the store.
    /// Absence is a normal outcome and is not modeled as an error;
    /// callers are expected to handle it.
    async fn get(&self, hash: Sha256) -> Result<Option<ContentValue>, StoreError>;

    /// Check whether a blob with the given hash exists in the store.
    ///
    /// Cheaper than `get` when the bytes aren't needed (e.g., deciding
    /// whether to upload a blob that the caller already has in memory).
    async fn exists(&self, hash: Sha256) -> Result<bool, StoreError>;
}

/// Typed ergonomics on top of [`ContentStore`].
///
/// The base trait deals in raw bytes (`ContentValue`) and raw hashes
/// (`Sha256`). This extension trait provides typed methods that consume
/// and produce values implementing [`Content`], with compile-time tracking
/// of what a hash is a hash *of* via [`ContentHash<T>`].
///
/// Blanket-implemented for any `ContentStore`: consumers that import
/// `ContentStoreExt` alongside `ContentStore` get the typed methods
/// automatically, regardless of which backend is behind the trait object.
#[async_trait]
pub trait ContentStoreExt: ContentStore {
    /// Store a typed content value and return its typed hash.
    ///
    /// The value is encoded via [`Content::to_content_bytes`], hashed,
    /// and stored. The returned [`ContentHash<T>`] carries the content
    /// type in its phantom parameter, so it cannot be accidentally used
    /// where a hash of a different content type is expected.
    async fn put_typed<T: Content + Sync>(
        &self,
        content: &T,
    ) -> Result<ContentHash<T>, StoreError> {
        let value = ContentValue::from(content);
        let hash = ContentHash::from_digest_unchecked(value.digest());
        self.put(&value).await?;
        Ok(hash)
    }

    /// Retrieve a typed content value by its typed hash.
    ///
    /// Returns `None` if no blob with this hash exists. Returns
    /// [`StoreError::Decode`] if the bytes exist but don't decode as `T`
    /// — usually a sign that the caller is asking for the wrong type, or
    /// that content at this hash was written under an incompatible schema.
    async fn get_typed<T: Content>(&self, hash: ContentHash<T>) -> Result<Option<T>, StoreError> {
        let Some(value) = self.get(hash.as_digest()).await? else {
            return Ok(None);
        };
        let decoded = value.decode::<T>()?;
        Ok(Some(decoded))
    }
}

impl<S: ContentStore + ?Sized> ContentStoreExt for S {}

#[cfg(test)]
mod tests {
    use crate::content::{ContentStore, ContentStoreExt};
    use crate::error::StoreError;

    use philharmonic_types::{CanonicalJson, ContentHash, ContentValue, Sha256};

    use std::collections::HashMap;
    use std::sync::Mutex;

    use async_trait::async_trait;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum ContentCall {
        Put { hash: Sha256 },
        Get { hash: Sha256 },
        Exists { hash: Sha256 },
    }

    struct MockContentStore {
        blobs: Mutex<HashMap<Sha256, Vec<u8>>>,
        calls: Mutex<Vec<ContentCall>>,
    }

    impl MockContentStore {
        fn new() -> Self {
            Self {
                blobs: Mutex::new(HashMap::new()),
                calls: Mutex::new(Vec::new()),
            }
        }

        fn stored_bytes(&self, hash: Sha256) -> Option<Vec<u8>> {
            self.blobs.lock().unwrap().get(&hash).cloned()
        }

        fn calls(&self) -> Vec<ContentCall> {
            self.calls.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl ContentStore for MockContentStore {
        async fn put(&self, value: &ContentValue) -> Result<(), StoreError> {
            self.calls.lock().unwrap().push(ContentCall::Put {
                hash: value.digest(),
            });
            self.blobs
                .lock()
                .unwrap()
                .insert(value.digest(), value.bytes().to_vec());
            Ok(())
        }

        async fn get(&self, hash: Sha256) -> Result<Option<ContentValue>, StoreError> {
            self.calls.lock().unwrap().push(ContentCall::Get { hash });
            let maybe = self
                .blobs
                .lock()
                .unwrap()
                .get(&hash)
                .cloned()
                .map(|bytes| ContentValue::from_parts_unchecked(hash, bytes));
            Ok(maybe)
        }

        async fn exists(&self, hash: Sha256) -> Result<bool, StoreError> {
            self.calls
                .lock()
                .unwrap()
                .push(ContentCall::Exists { hash });
            Ok(self.blobs.lock().unwrap().contains_key(&hash))
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn put_typed_stores_canonical_bytes() {
        let store = MockContentStore::new();
        let content = CanonicalJson::from_bytes(br#"{"z":3,"a":1}"#).unwrap();

        let hash = store.put_typed(&content).await.unwrap();

        assert_eq!(hash.as_digest(), content.digest());
        let stored = store.stored_bytes(content.digest()).unwrap();
        assert_eq!(stored, content.as_bytes());
        assert_eq!(
            store.calls(),
            vec![ContentCall::Put {
                hash: content.digest()
            }]
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn get_typed_decodes_existing_value() {
        let store = MockContentStore::new();
        let content = CanonicalJson::from_bytes(br#"{"k":"v"}"#).unwrap();
        let hash = store.put_typed(&content).await.unwrap();

        let got = store.get_typed::<CanonicalJson>(hash).await.unwrap();

        assert_eq!(got, Some(content));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn get_typed_returns_none_when_missing() {
        let store = MockContentStore::new();
        let content = CanonicalJson::from_bytes(br#"{"missing":true}"#).unwrap();
        let missing = ContentHash::<CanonicalJson>::from_digest_unchecked(content.digest());

        let got = store.get_typed::<CanonicalJson>(missing).await.unwrap();

        assert!(got.is_none());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn get_typed_returns_decode_error_for_invalid_bytes() {
        let store = MockContentStore::new();
        let value = ContentValue::new(vec![0x80, 0x81, 0x82]);
        store.put(&value).await.unwrap();
        let hash = ContentHash::<CanonicalJson>::from_digest_unchecked(value.digest());

        let err = store.get_typed::<CanonicalJson>(hash).await.unwrap_err();

        assert!(matches!(err, StoreError::Decode(_)));
    }
}
