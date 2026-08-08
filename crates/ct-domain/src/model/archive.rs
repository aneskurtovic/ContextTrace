//! Keeping a session after its log is gone.
//!
//! # What is actually at risk
//!
//! Not compaction. A compaction destroys the *agent's* access to earlier
//! content, but every pre-compaction record stays in the append-only log --
//! which is the only reason a compaction autopsy can exist at all. What no
//! amount of reconstruction survives is **deletion**: a log rotated by the
//! harness, pruned, or lost with a wiped home directory takes its evidence with
//! it. An archive is the only way a session outlives its log.
//!
//! # Why the archive stores records rather than findings
//!
//! An export of this tool's conclusions would preserve the answers this build
//! happens to produce today. Storing the records themselves preserves the
//! evidence, so a later build with better reconstruction reaches better answers
//! from the same archive -- and, more importantly, so that every existing view
//! reads an archived session through exactly the same parser it reads a live
//! one through. That is what makes "reads identically from either" a structural
//! property rather than a claim someone has to keep true by hand.
//!
//! # The failure this module is shaped to prevent
//!
//! A copy that silently substitutes for evidence is this project's central
//! failure mode wearing a database schema. So an archive is never authoritative
//! while the log it came from is present, every entry records where it came
//! from and what was done to it on the way in, and an archive that has drifted
//! from its source says so rather than answering as though it had not.

use crate::model::identity::SessionId;
use crate::model::session::{AgentKind, SessionDescriptor};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Whether an archived copy still holds the credentials its source held.
///
/// A sum type with no default that means "unknown". An archive concentrates by
/// construction what was previously scattered across two home directories: one
/// place holding every prompt, tool output and credential a machine has ever
/// produced is a materially better target than the logs it came from. So
/// retaining credentials is a decision, it is recorded per session, and the
/// path of least resistance is the safe one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RedactionMode {
    /// Recognised credential shapes were replaced on the way in. The default.
    Redacted,
    /// Records were copied byte for byte, credentials included. Only ever an
    /// explicit request, and recorded here so a reader of the archive knows
    /// what it is holding without having to scan it again.
    Raw,
}

impl RedactionMode {
    pub fn label(self) -> &'static str {
        match self {
            RedactionMode::Redacted => "redacted",
            RedactionMode::Raw => "raw",
        }
    }
}

/// One archived session, as the manifest records it.
///
/// Carries both digests deliberately. `source_digest` answers "is the log still
/// the file this copy was taken from"; `archived_digest` answers "is this copy
/// still what was written". They fail independently -- a log that grew since
/// ingest is a stale archive, a copy that no longer matches its digest is a
/// damaged one -- and a single digest could not tell a reader which had
/// happened.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArchiveEntry {
    /// The session as its agent's adapter described it at ingest, including the
    /// original path. This is the "where it came from" the archive owes every
    /// record it holds: an entry that could not name its source would be a copy
    /// with no provenance, which is indistinguishable from a fabrication.
    pub descriptor: SessionDescriptor,
    pub archived_at: DateTime<Utc>,
    pub redaction: RedactionMode,
    /// Records copied. A count, so it is comparable across redaction modes --
    /// redaction changes bytes but never the number of records.
    pub records: u64,
    pub source_bytes: u64,
    /// Hex SHA-256 of the source log's bytes as they were read.
    pub source_digest: String,
    /// Bytes written here. Equal to `source_bytes` exactly when nothing was
    /// replaced, which is the ordinary case: most sessions carry no credentials
    /// at all, and for those the archive is a byte-identical copy.
    pub archived_bytes: u64,
    pub archived_digest: String,
    /// Records in which at least one value was replaced.
    pub redacted_records: u64,
    /// Values replaced across all records. At least `redacted_records` whenever
    /// either is non-zero, since one record can hold several.
    pub redacted_values: u64,
}

impl ArchiveEntry {
    /// Whether this copy differs from the log it was taken from.
    ///
    /// The honest reading of a redacted archive: it is not a facsimile, and
    /// anything computed from it that depends on exact content -- a token count
    /// over a credential-bearing record, a fingerprint -- will differ from the
    /// same computation over the log. Where the log is present that never
    /// matters, because the log answers. Where it is gone, this is the flag
    /// that keeps the difference stated rather than discovered.
    pub fn differs_from_source(&self) -> bool {
        self.redacted_values > 0
    }

    pub fn id(&self) -> &SessionId {
        &self.descriptor.id
    }

    pub fn agent(&self) -> AgentKind {
        self.descriptor.agent
    }
}

/// What checking an archived session against the world found.
///
/// Four outcomes rather than a boolean and a message, because they call for
/// different actions and a caller must not have to parse prose to tell them
/// apart: an intact archive needs nothing, a changed source needs a re-ingest,
/// a damaged copy needs a re-ingest *and* means the previous one cannot be
/// trusted, and a vanished source means this copy is now the only evidence
/// there is -- the case the whole feature exists for, and the one where saying
/// "verified" without qualification would be worst.
#[derive(Debug, Clone, PartialEq)]
pub enum ArchiveIntegrity {
    /// The source log is present, unchanged, and the copy matches its digest.
    Intact,
    /// The copy matches its digest, but the log has changed since ingest --
    /// almost always because the session continued. The archive is a valid copy
    /// of an earlier state, and re-ingesting brings it current.
    SourceChanged {
        recorded_digest: String,
        current_digest: String,
        recorded_bytes: u64,
        current_bytes: u64,
    },
    /// The source log is gone. This copy is the only remaining evidence, and
    /// nothing can re-derive it.
    SourceGone { archive_matches_digest: bool },
    /// The archived file no longer matches the digest recorded for it. Whatever
    /// this copy now says, it is not what was archived.
    ArchiveDamaged {
        recorded_digest: String,
        current_digest: String,
    },
}

impl ArchiveIntegrity {
    /// Whether the archived copy can still be read as the thing it claims to
    /// be. False only for a damaged copy: a changed or vanished source says
    /// something about the *world*, not about this file.
    pub fn copy_is_sound(&self) -> bool {
        match self {
            ArchiveIntegrity::Intact | ArchiveIntegrity::SourceChanged { .. } => true,
            ArchiveIntegrity::SourceGone {
                archive_matches_digest,
            } => *archive_matches_digest,
            ArchiveIntegrity::ArchiveDamaged { .. } => false,
        }
    }

    /// Whether re-reading the log would produce a better copy than this one.
    /// False when there is no log left to read.
    pub fn rebuildable(&self) -> bool {
        match self {
            ArchiveIntegrity::Intact => false,
            ArchiveIntegrity::SourceChanged { .. } | ArchiveIntegrity::ArchiveDamaged { .. } => {
                true
            }
            ArchiveIntegrity::SourceGone { .. } => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_damaged_copy_is_the_only_outcome_that_makes_the_archive_unreadable() {
        assert!(ArchiveIntegrity::Intact.copy_is_sound());
        assert!(ArchiveIntegrity::SourceChanged {
            recorded_digest: "a".into(),
            current_digest: "b".into(),
            recorded_bytes: 1,
            current_bytes: 2,
        }
        .copy_is_sound());
        assert!(!ArchiveIntegrity::ArchiveDamaged {
            recorded_digest: "a".into(),
            current_digest: "b".into(),
        }
        .copy_is_sound());
    }

    #[test]
    fn a_vanished_source_is_not_rebuildable_however_sound_the_copy() {
        // The case the feature exists for. There is nothing left to re-read, so
        // offering a rebuild here would promise something impossible.
        let gone = ArchiveIntegrity::SourceGone {
            archive_matches_digest: true,
        };
        assert!(gone.copy_is_sound());
        assert!(!gone.rebuildable());
    }

    #[test]
    fn an_archive_reports_divergence_only_when_something_was_actually_replaced() {
        // Redaction is a no-op on a session holding no credentials, which is
        // most of them -- so "redacted" must not by itself mean "differs".
        let entry = |redacted_values| ArchiveEntry {
            descriptor: SessionDescriptor {
                id: SessionId::new("s").expect("a non-blank id"),
                agent: AgentKind::Codex,
                path: "p".into(),
                size_bytes: 0,
                project: None,
                started_at: None,
                last_activity: None,
                thread_role: Default::default(),
            },
            archived_at: DateTime::UNIX_EPOCH,
            redaction: RedactionMode::Redacted,
            records: 3,
            source_bytes: 10,
            source_digest: "a".into(),
            archived_bytes: 10,
            archived_digest: "a".into(),
            redacted_records: 0,
            redacted_values,
        };
        assert!(!entry(0).differs_from_source());
        assert!(entry(1).differs_from_source());
    }
}
