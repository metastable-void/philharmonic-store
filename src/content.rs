use std::{borrow::Cow, convert::Infallible};

use async_trait::async_trait;
use philharmonic_types::{CanonicalJson, Content, Sha256};

use crate::StoreError;

pub struct UntypedContent;

const SLICE: [u8; 0] = [];

impl Content for UntypedContent {
    type Error = Infallible;
    fn from_bytes(_bytes: &[u8]) -> Result<Self, Self::Error> {
        Ok(UntypedContent)
    }

    fn to_bytes(&'_ self) -> std::borrow::Cow<'_, [u8]> {
        Cow::Borrowed(&SLICE)
    }
}

#[async_trait]
pub trait ContentStore: Send + Sync {
    async fn put(&self, content: &[u8]) -> Result<Sha256<[u8]>, StoreError>;

    async fn get(&self, hash: Sha256<[u8]>) -> Result<Option<Vec<u8>>, StoreError>;
}
