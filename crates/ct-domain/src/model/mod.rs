//! Entities, value objects and aggregates.
//!
//! Reading order, innermost first: [`identity`] and [`provenance`] define the
//! value objects everything else is built from; [`tokens`] adds the counting
//! vocabulary; [`event`] and [`session`] are the parsed-session entities; and
//! [`context`] holds the [`ContextSnapshot`](context::ContextSnapshot)
//! aggregate that the whole product exists to produce.

pub mod analysis;
pub mod compaction_diff;
pub mod context;
pub mod event;
pub mod filter;
pub mod identity;
pub mod provenance;
pub mod session;
pub mod tokens;
