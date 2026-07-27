//! # ContextTrace domain
//!
//! The centre of the hexagon. Everything here is expressed in the ubiquitous
//! language of agent-context debugging -- Session, Turn, ContextSnapshot,
//! ContextItem, Contributor, Compaction, Provenance -- and nothing here knows
//! that Codex, Claude Code, JSONL, SQLite or a terminal exist.
//!
//! ## Layout
//!
//! - [`model`]    -- entities, value objects and the [`AgentSession`] aggregate.
//! - [`ports`]    -- traits the outside world must satisfy to be usable here.
//! - [`services`] -- domain services: logic that belongs to no single entity.
//!
//! ## The two invariants worth knowing
//!
//! **Confidence never launders upward.** Combining an observed fact with an
//! estimated one yields an estimate ([`Confidence::weakest`]). There is no path
//! by which a guess becomes a measurement.
//!
//! **A context snapshot always adds up.** [`ContextSnapshot`] can only be built
//! through [`services::TokenCalibrator`], which guarantees that the sum of item
//! token counts plus the unattributed residual equals the reported total. An
//! inconsistent breakdown is not something a future adapter can forget to
//! prevent -- it is unrepresentable.

pub mod model;
pub mod ports;
pub mod services;

pub use model::context::{
    CategoryBreakdown, CompactionEvent, ContextCategory, ContextItem, ContextSnapshot,
    ContextSource, Contributor,
};
pub use model::event::{Event, EventKind, MessageRole};
pub use model::filter::{
    CompositionReport, ContributorReport, FilterParseError, FilteredView, ItemFilter, SourceKind,
    SourcePattern, ViewHeader,
};
pub use model::identity::{ContextItemId, EventId, FileId, SessionId, TurnNumber};
pub use model::provenance::{Confidence, Provenance, SourceRef};
pub use model::session::{AgentKind, AgentSession, SessionDescriptor, SessionMetadata, Turn};
pub use model::tokens::{TokenCount, TokenUsage};
