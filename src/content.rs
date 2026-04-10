use async_trait::async_trait;
use philharmonic_types::{CanonicalJson, Sha256};

use crate::StoreError;

#[async_trait]
pub trait ContentStore: Send + Sync {
    async fn put_canonical_json(&self, json: &CanonicalJson) -> Result<Sha256, StoreError>;

    async fn get_canonical_json(&self, hash: Sha256) -> Result<Option<CanonicalJson>, StoreError>;
}
