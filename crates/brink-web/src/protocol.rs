//! Wire envelopes for the editor session protocol
//! (`docs/editor-worker-spec.md` §5).
//!
//! **This module is the source of truth for the protocol's shapes**
//! (spec §5.4): the worker transport (`postMessage`) and any future
//! native transport (a sidecar byte stream) must carry identical JSON,
//! so the shapes are defined here once, with hand-maintained TypeScript
//! mirrors in `packages/wasm-types/src/index.ts` (the existing
//! house pattern for wire shapes). The golden strings in this module's
//! tests are duplicated verbatim in the TS suite
//! (`packages/ink-editor/src/__tests__/worker-protocol.test.ts`) —
//! change one side and the other's pin fails.
//!
//! Every payload is JSON-serializable by construction (spec §5.1): no
//! maps, no binary views, no `undefined`-bearing shapes. `serde_json`
//! round-trips are the compatibility bar, not structured clone.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Client-assigned id correlating a `Query` with its `Result`/`Error`.
pub type RequestId = u32;

/// A single text edit in UTF-16 document coordinates — the same shape
/// the delta-ingress endpoint (`applyEditsDocument`) accepts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EditSpan {
    pub from: u32,
    pub to: u32,
    pub insert: String,
}

/// A named mutation on the session's config or file surface — the
/// ordered, fire-and-forget half of the protocol. `method`/`args`
/// address the session facade's own surface (e.g. `setDialect`,
/// `updateFile`), so the envelope does not enumerate every op.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionOp {
    pub method: String,
    pub args: Vec<Value>,
}

/// Scheduling class for a query (spec §6): interactive queries run
/// before background pulls; only background queries coalesce or drop.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum QueryPriority {
    Interactive,
    Background,
}

/// Main-thread → session-host messages.
///
/// The mutation stream (`Edit`/`Push`/`Config`/`Files`) is strictly
/// ordered and applied FIFO before any query runs; queries are
/// unordered beyond their priority class (spec §5.2, §6).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum SessionRequest {
    /// Bounded edit list against `doc`, advancing it to `doc_version`.
    #[serde(rename_all = "camelCase")]
    Edit {
        doc: u32,
        doc_version: u64,
        edits: Vec<EditSpan>,
    },
    /// Full-text fallback push (multi-cursor transactions, recovery).
    #[serde(rename_all = "camelCase")]
    Push {
        doc: u32,
        doc_version: u64,
        source: String,
    },
    /// Config-surface mutation (dialect, lints, host manifest, …).
    Config { op: SessionOp },
    /// File-surface mutation (add/update/remove/rename/external change).
    Files { op: SessionOp },
    /// A read query against the session facade.
    ///
    /// `coalesce_key`: background-only supersession handle (spec §6) —
    /// a queued background query is dropped when a newer query with the
    /// SAME key sits behind it. `None` never coalesces; the key is
    /// client-chosen (e.g. `"tokens:refined:1"`), never derived from
    /// `method` alone, because same-method queries with different args
    /// (per-segment slice pulls) are distinct work.
    #[serde(rename_all = "camelCase")]
    Query {
        id: RequestId,
        priority: QueryPriority,
        #[serde(skip_serializing_if = "Option::is_none")]
        doc: Option<u32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        doc_version: Option<u64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        coalesce_key: Option<String>,
        method: String,
        args: Vec<Value>,
    },
    /// Best-effort cancellation: removes the queued query with this id.
    /// Cannot interrupt a running query (spec §6).
    Cancel { id: RequestId },
}

/// Session-host → main-thread messages.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum SessionResponse {
    /// A mutation was applied (or refused: `applied: false` — e.g. a
    /// malformed edit list or a read-only target; the client falls back
    /// to a full `Push`).
    #[serde(rename_all = "camelCase")]
    Ack {
        doc: u32,
        doc_version: u64,
        applied: bool,
    },
    /// A query's value. `doc_version` is the doc's version at execution
    /// time (absent for doc-less queries); `config_epoch` stamps which
    /// config generation produced it. Staleness policy is the
    /// *consumer's* call (spec §5.3) — the host never withholds a
    /// computed result.
    #[serde(rename_all = "camelCase")]
    Result {
        id: RequestId,
        #[serde(skip_serializing_if = "Option::is_none")]
        doc_version: Option<u64>,
        config_epoch: u64,
        value: Value,
    },
    /// A query failed or was dropped before running. Dropped queries use
    /// the `dropped:` message prefix (`dropped:superseded`,
    /// `dropped:stale`) so clients can tell policy drops from faults.
    Error { id: RequestId, message: String },
    /// Host-initiated event (file-change egress, config warnings, …).
    Event { event: Value },
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Golden wire strings — duplicated verbatim in
    /// `packages/ink-editor/src/__tests__/worker-protocol.test.ts`.
    /// A change here must be mirrored there (and vice versa) or that
    /// suite's pin fails; this is the cross-language shape lock.
    const GOLDEN_EDIT: &str =
        r#"{"kind":"edit","doc":1,"docVersion":7,"edits":[{"from":10,"to":12,"insert":"ab"}]}"#;
    const GOLDEN_QUERY: &str = r#"{"kind":"query","id":3,"priority":"background","doc":1,"docVersion":7,"coalesceKey":"tokens:refined:1","method":"getSegmentSemanticTokensDoc","args":[1,"4:0"]}"#;
    const GOLDEN_ACK: &str = r#"{"kind":"ack","doc":1,"docVersion":7,"applied":true}"#;
    const GOLDEN_RESULT: &str =
        r#"{"kind":"result","id":3,"docVersion":7,"configEpoch":2,"value":[]}"#;
    const GOLDEN_ERROR: &str = r#"{"kind":"error","id":9,"message":"dropped:superseded"}"#;

    fn assert_round_trip<T>(value: &T, golden: &str)
    where
        T: Serialize + for<'de> Deserialize<'de> + PartialEq + core::fmt::Debug,
    {
        let json = serde_json::to_string(value).expect("serializes");
        assert_eq!(json, golden, "wire shape drifted from the golden pin");
        let back: T = serde_json::from_str(&json).expect("deserializes");
        assert_eq!(&back, value, "round trip must be lossless");
    }

    #[test]
    fn edit_round_trips_to_golden() {
        assert_round_trip(
            &SessionRequest::Edit {
                doc: 1,
                doc_version: 7,
                edits: vec![EditSpan {
                    from: 10,
                    to: 12,
                    insert: "ab".into(),
                }],
            },
            GOLDEN_EDIT,
        );
    }

    #[test]
    fn query_round_trips_to_golden() {
        assert_round_trip(
            &SessionRequest::Query {
                id: 3,
                priority: QueryPriority::Background,
                doc: Some(1),
                doc_version: Some(7),
                coalesce_key: Some("tokens:refined:1".into()),
                method: "getSegmentSemanticTokensDoc".into(),
                args: vec![Value::from(1), Value::from("4:0")],
            },
            GOLDEN_QUERY,
        );
    }

    #[test]
    fn responses_round_trip_to_goldens() {
        assert_round_trip(
            &SessionResponse::Ack {
                doc: 1,
                doc_version: 7,
                applied: true,
            },
            GOLDEN_ACK,
        );
        assert_round_trip(
            &SessionResponse::Result {
                id: 3,
                doc_version: Some(7),
                config_epoch: 2,
                value: Value::Array(vec![]),
            },
            GOLDEN_RESULT,
        );
        assert_round_trip(
            &SessionResponse::Error {
                id: 9,
                message: "dropped:superseded".into(),
            },
            GOLDEN_ERROR,
        );
    }

    #[test]
    fn optional_fields_are_omitted_not_null() {
        let q = SessionRequest::Query {
            id: 1,
            priority: QueryPriority::Interactive,
            doc: None,
            doc_version: None,
            coalesce_key: None,
            method: "listFiles".into(),
            args: vec![],
        };
        let json = serde_json::to_string(&q).expect("serializes");
        assert!(
            !json.contains("null"),
            "absent optionals must be omitted so JS mirrors see missing keys, not null: {json}"
        );
    }
}
