//! Static contract for result-supersession migration error classification.
//!
//! Migration reapplication classifies dangling predecessor references before it
//! evaluates cycle reachability. That ordering is intentional: the root-based
//! reachability check would also leave a dangling component unreachable, so the
//! earlier guard must retain ownership of SQLSTATE 23503 while true cycles use
//! SQLSTATE 23514.

const RESULT_SNAPSHOT_MIGRATION: &str = include_str!("../migrations/0007_result_snapshot.sql");
const REAPPLY_GUARD_START: &str = "-- Supersession is an immutable backward link.";
const PREDECESSOR_TRIGGER_START: &str =
    "CREATE OR REPLACE FUNCTION require_result_snapshot_supersession_predecessor()";

#[test]
fn dangling_predecessor_guard_runs_before_cycle_guard() {
    let reapply_guard_start = RESULT_SNAPSHOT_MIGRATION
        .find(REAPPLY_GUARD_START)
        .expect("migration must retain the supersession reapply guard block");
    let predecessor_trigger_start = RESULT_SNAPSHOT_MIGRATION[reapply_guard_start..]
        .find(PREDECESSOR_TRIGGER_START)
        .map(|offset| reapply_guard_start + offset)
        .expect("reapply guard block must remain before the predecessor trigger");
    let reapply_guard = &RESULT_SNAPSHOT_MIGRATION[reapply_guard_start..predecessor_trigger_start];

    let dangling_guard = reapply_guard
        .find("result snapshot supersession predecessor must already exist")
        .expect("reapply guard must retain the dangling-predecessor failure");
    let dangling_sqlstate = reapply_guard[dangling_guard..]
        .find("USING ERRCODE = '23503'")
        .map(|offset| dangling_guard + offset)
        .expect("reapply dangling-predecessor guard must retain SQLSTATE 23503");
    let cycle_guard = reapply_guard
        .find("WITH RECURSIVE reachable_supersession_result AS")
        .expect("reapply guard must retain the root-based cycle guard");
    let cycle_error = reapply_guard[cycle_guard..]
        .find("result snapshot supersession lineage must be acyclic")
        .map(|offset| cycle_guard + offset)
        .expect("reapply cycle guard must retain its explicit failure");
    let cycle_sqlstate = reapply_guard[cycle_error..]
        .find("USING ERRCODE = '23514'")
        .map(|offset| cycle_error + offset)
        .expect("reapply cycle guard must retain SQLSTATE 23514");

    assert!(
        dangling_guard < dangling_sqlstate
            && dangling_sqlstate < cycle_guard
            && cycle_guard < cycle_error
            && cycle_error < cycle_sqlstate,
        "dangling predecessor classification must remain before cycle classification inside the reapply guard block"
    );
}
