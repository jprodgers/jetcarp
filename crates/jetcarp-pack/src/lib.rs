//! JETCARP packs, format 1.0 (`docs/src/reference/pack-format.md` is the normative text).
//!
//! A **pack** is a published, versioned collection of JETCARP data (patterns, ant runs, Quick
//! Start scenes, the tilings and rules they run on, and optionally search runs, media and
//! ratings) that people can use without JETCARP: every table is a Parquet file and every other
//! document is RFC 8785 canonical JSON, every file is named by its SHA-256, every version is
//! signed with an SSH signature, and every record's id is a hash of an identity text the pack
//! carries.
//!
//! | Module | What it does |
//! | --- | --- |
//! | [`json`] | canonical JSON documents and their limits |
//! | [`hash`] | `sha256:` object names |
//! | [`docs`] | manifests, indexes, refs, key statements |
//! | [`store`] | the store's layout: objects, refs, signatures, keys |
//! | [`sign`] | Ed25519 SSH signatures, the publisher's key |
//! | [`columns`], [`tables`] | the tables' columns and rows, and Arrow conversion |
//! | [`parquet_io`] | deterministic parts, and reading them with a hostile pack's limits |
//! | [`thrift`] | page-header and footer checks before decoding |
//! | [`publish`] | a library (and its search folders and media) into a store |
//! | [`read`] | a store or an export, checked: format, signatures, every hash |
//! | [`import`] | a checked version into a library |
//! | [`export`] | one version laid out for people |
//! | [`rows`] | library records as pack rows, and back |
//! | [`paths`] | keeping local paths out |
//!
//! Arrow and Parquet's Arrow API live only here, so builds that do not use packs (the macOS
//! screen saver, world-only scripting) never link them.

pub mod columns;
pub mod docs;
pub mod export;
pub mod hash;
pub mod import;
pub mod json;
pub mod parquet_io;
pub mod paths;
pub mod publish;
pub mod read;
pub mod rows;
pub mod sign;
pub mod store;
pub mod tables;
pub mod thrift;

/// The format's name in a manifest.
pub const FORMAT: &str = "jetcarp-pack";

/// The table schemes a manifest's `engine.schemes` names.
pub const TABLES_SCHEME: &str = "jetcarp-tables-1";
