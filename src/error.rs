use std::borrow::Cow;

#[derive(thiserror::Error, Debug)]
pub enum StoreError {
    #[error("Content Error: {0}")]
    Content(#[from] Box<dyn std::error::Error + Send + Sync + 'static>),

    #[error("Store Error: {0}")]
    Store(Cow<'static, str>),
}
