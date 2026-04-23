# philharmonic-store

Storage substrate traits for the Philharmonic workflow orchestration system.

This crate defines the interfaces; it does not implement them. The canonical
implementation is [`philharmonic-store-sqlx-mysql`][sqlx-mysql], which backs
the substrate onto MySQL-family databases via `sqlx`. Other backends
(in-memory, alternative SQL flavors) are possible but not provided here.

`philharmonic-store` depends only on [`philharmonic-types`][types] plus
`async-trait`, `serde`, and `thiserror`. No database drivers, no async
runtime, no SQL anywhere. Downstream crates that want to be
backend-agnostic depend only on this crate; crates that need an actual
running store depend on one of the backend crates as well.

## What's in the substrate

Three traits, one per storage concern:

- **`ContentStore`** — content-addressed blob storage. Write bytes, get
  back a SHA-256 hash; read bytes back by hash. Grow-only: writes are
  idempotent, there is no delete.
- **`IdentityStore`** — minting and resolution of `(internal, public)`
  UUID pairs. Internal IDs are UUIDv7 (time-ordered, for internal
  storage); public IDs are UUIDv4 (opaque, for external references).
  Append-only.
- **`EntityStore`** — entities and their append-only revision logs.
  Each revision carries content-hash, entity-reference, and scalar
  attributes per its entity kind's declared slots.

Each trait has a typed extension trait (`ContentStoreExt`,
`IdentityStoreExt`, `EntityStoreExt`) providing phantom-typed ergonomics
on top of the object-safe base. An umbrella `StoreExt` trait adds
cross-concern conveniences like `create_entity_minting` for callers that
hold a combined implementation.

## Design properties

**Grow-only.** Every substrate operation either adds data or reads it;
nothing is ever modified or deleted. This collapses most concurrency
concerns: no coordination needed for read-after-write, no transactional
API at the trait layer, no versioning machinery. Consumers that want
semantic deletion (tombstoning a workflow, retiring a template) express
it as a new revision with a deletion scalar, not as a removal from the
store.

**Object-safe.** Base traits can be held as `&dyn ContentStore` etc.,
enabling runtime backend selection and straightforward mocking for
tests. Extension traits provide typed methods via blanket impls; they
compose with the object-safe base without adding vtable entries.

**Backend-agnostic.** The trait signatures mention no SQL types, no
database driver errors, no backend-specific connection abstractions.
Backend-specific failures reach consumers as `StoreError::Backend` with
a human-readable message and a retry-ability hint.

**LCD MySQL in the canonical backend.** The SQL implementation targets
MySQL 8, MariaDB 10.5+, Amazon Aurora MySQL, and TiDB, using only
features common to all of them. Timestamps are `BIGINT` milliseconds
since epoch, UUIDs are `BINARY(16)`, hashes are `BINARY(32)`, no JSON
columns, no vendor-specific operators. Other backends are free to make
their own LCD choices.

## Status

Early development. API shape is stabilizing but not yet stable. Breaking
changes in 0.x releases are possible; semver guarantees begin at 1.0.

Used by the [Philharmonic][philharmonic] workflow orchestration system
and designed to be generally useful for any system that wants
content-addressed, entity-centric, append-only storage with a clean
separation between substrate and domain concerns.

[types]: https://github.com/metastable-void/philharmonic-types
[sqlx-mysql]: https://github.com/metastable-void/philharmonic-store-sqlx-mysql
[philharmonic]: https://github.com/metastable-void/philharmonic

## License

**This crate is dual-licensed under `Apache-2.0 OR MPL-2.0`**;
either license is sufficient; choose whichever fits your project.

**Rationale**: We generally want our reusable Rust crates to be
under a license permissive enough to be friendly for the Rust
community as a whole, while maintaining GPL-2.0 compatibility via
the MPL-2.0 arm. This is FSF-safer for everyone than `MIT OR Apache-2.0`,
still being permissive. **This is the standard licensing** for our reusable
Rust crate projects. Someone's `GPL-2.0-or-later` project should not be
forced to drop the `GPL-2.0` option because of our crates,
while `Apache-2.0` is the non-copyleft (permissive) license recommended
by the FSF, which we base our decisions on.

## Contributing

This crate is developed as a submodule of the Philharmonic
workspace. Workspace-wide development conventions — git workflow,
script wrappers, Rust code rules, versioning, terminology — live
in the workspace meta-repo at
[metastable-void/philharmonic-workspace](https://github.com/metastable-void/philharmonic-workspace),
authoritatively in its
[`CONTRIBUTING.md`](https://github.com/metastable-void/philharmonic-workspace/blob/main/CONTRIBUTING.md).
