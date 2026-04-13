use async_trait::async_trait;
use philharmonic_types::{CanonicalJson, Content, Sha256};

use crate::StoreError;

#[async_trait]
pub trait ContentStore: Send + Sync {
    async fn put<T: Content>(&self, content: &T) -> Result<Sha256<T>, StoreError>;

    async fn get<T: Content>(&self, hash: Sha256<T>) -> Result<Option<CanonicalJson>, StoreError>;
}
