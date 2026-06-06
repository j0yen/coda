//! AC2: SessionLog, SummaryState, DebtClass, SweepAction, SweepPlan are public
//! and serde-(de)serializable; a round-trip test covers each.

use std::path::PathBuf;

use coda::{
    sweep, DebtClass, RawLog, SessionLog, SummaryState, SweepAction, SweepPlan,
};

fn roundtrip_json<T: serde::Serialize + serde::de::DeserializeOwned + std::fmt::Debug + PartialEq>(
    value: &T,
) {
    let json = serde_json::to_string(value).expect("serialize");
    let decoded: T = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(value, &decoded, "JSON round-trip failed for {value:?}");
}

#[test]
fn test_summary_state_roundtrip() {
    roundtrip_json(&SummaryState::Present);
    roundtrip_json(&SummaryState::Missing);
}

#[test]
fn test_debt_class_roundtrip() {
    roundtrip_json(&DebtClass::Active);
    roundtrip_json(&DebtClass::Fresh { age_secs: 42 });
    roundtrip_json(&DebtClass::Orphaned { age_secs: 999 });
    roundtrip_json(&DebtClass::Settled);
}

#[test]
fn test_sweep_action_roundtrip() {
    roundtrip_json(&SweepAction::Render {
        path: PathBuf::from("/sessions/a.ndjson"),
    });
    roundtrip_json(&SweepAction::Skip {
        path: PathBuf::from("/sessions/b.ndjson"),
        reason: "hook may still fire".to_string(),
    });
    roundtrip_json(&SweepAction::NoOp {
        path: PathBuf::from("/sessions/c.ndjson"),
    });
}

#[test]
fn test_session_log_roundtrip() {
    let log = SessionLog {
        path: PathBuf::from("/sessions/test.ndjson"),
        summary: SummaryState::Missing,
        age_secs: 300,
        is_active: false,
        debt: DebtClass::Orphaned { age_secs: 300 },
    };
    roundtrip_json(&log);
}

#[test]
fn test_sweep_plan_roundtrip() {
    let now = 2000u64;
    let logs = vec![
        RawLog {
            path: PathBuf::from("/sessions/orphaned.ndjson"),
            has_summary: false,
            mtime_secs: now - 300,
        },
        RawLog {
            path: PathBuf::from("/sessions/settled.ndjson"),
            has_summary: true,
            mtime_secs: now - 1000,
        },
    ];
    let plan = sweep(&logs, None, now, 120);
    roundtrip_json(&plan);
}

#[test]
fn test_raw_log_roundtrip() {
    let raw = RawLog {
        path: PathBuf::from("/sessions/x.ndjson"),
        has_summary: true,
        mtime_secs: 1234567890,
    };
    roundtrip_json(&raw);
}
