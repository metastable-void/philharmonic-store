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
    async fn get_typed<T: Content>(
        &self,
        hash: ContentHash<T>,
    ) -> Result<Option<T>, StoreError> {
        let Some(value) = self.get(hash.as_digest()).await? else {
            return Ok(None);
        };
        let decoded = value.decode::<T>()?;
        Ok(Some(decoded))
    }
}

impl<S: ContentStore + ?Sized> ContentStoreExt for S {}
