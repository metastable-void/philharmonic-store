//! Storage substrate traits for the Philharmonic workflow orchestration system.
//!
//! This crate defines the interfaces — no implementation. The canonical
//! implementation is `philharmonic-store-sqlx-mysql`, which backs the
//! substrate onto MySQL-family databases via `sqlx`.
//!
//! # Substrate concerns
//!
//! The substrate has three concerns, each a trait:
//!
//! * [`ContentStore`]: content-addressed blob storage. Write once, read by
//!   hash, never update. Grow-only.
//! * [`IdentityStore`]: minting and resolving `(internal, public)` UUID
//!   identity pairs. Append-only.
//! * [`EntityStore`]: entities and their append-only revision logs,
//!   including content-ref, entity-ref, and scalar attributes per revision.
//!
//! All three stores are grow-only: writes never modify or delete existing
//! data, so they require no transactional coordination at the trait layer.
//! Backend implementations may use transactions internally for multi-row
//! operations (like appending a revision with its attributes), but this is
//! an implementation concern, not an interface one.
//!
//! # Type safety
//!
//! Each trait has an extension trait (`ContentStoreExt`, `IdentityStoreExt`,
//! `EntityStoreExt`) providing typed ergonomics on top of the object-safe
//! base trait. Consumers who want typed operations import the extension
//! trait alongside the base; consumers who want dyn-dispatch over backends
//! hold `&dyn ContentStore` and similar.
//!
//! The umbrella [`StoreExt`] trait adds cross-concern conveniences like
//! `create_entity_minting`, which mints an identity and creates an entity
//! in one call.

pub(crate) mod content;
pub(crate) mod entity;
pub(crate) mod error;
pub(crate) mod ext;
pub(crate) mod identity;
pub(crate) mod revision;

pub use content::{ContentStore, ContentStoreExt};
pub use entity::{EntityRow, EntityStore, EntityStoreExt};
pub use error::{BackendError, StoreError};
pub use ext::StoreExt;
pub use identity::{IdentityStore, IdentityStoreExt};
pub use revision::{EntityRefValue, RevisionInput, RevisionRef, RevisionRow};
