//! Static contract for result-supersession migration error classification.
//!
//! Migration reapplication classifies dangling predecessor references before it
//! evaluates cycle reachability. That ordering is intentional: the root-based
//! reachability check would also leave a dangling component unreachable, so the
//! earlier guard must retain ownership of SQLSTATE 23503 while true cycles use
//! SQLSTATE 23514.

const RESULT_SNAPSHOT_MIGRATION: &str = include_str!("../migrations/0007_result_snapshot.sql");

#[test]
fn dangling_predecessor_guard_runs_before_cycle_guard() {
    let dangling_guard = RESULT_SNAPSHOT_MIGRATION
        .find("result snapshot supersession predecessor must already exist")
        .expect("migration must retain the dangling-predecessor guard");
    let dangling_sqlstate = RESULT_SNAPSHOT_MIGRATION[dangling_guard..]
        .find("USING ERRCODE = '23503'")
        .map(|offset| dangling_guard + offset)
        .expect("dangling-predecessor guard must retain SQLSTATE 23503");
    let cycle_guard = RESULT_SNAPSHOT_MIGRATION
        .find("WITH RECURSIVE reachable_supersession_result AS")
        .expect("migration must retain the root-based cycle guard");
    let cycle_error = RESULT_SNAPSHOT_MIGRATION[cycle_guard..]
        .find("result snapshot supersession lineage must be acyclic")
        .map(|offset| cycle_guard + offset)
        .expect("cycle guard must retain its explicit failure");
    let cycle_sqlstate = RESULT_SNAPSHOT_MIGRATION[cycle_error..]
        .find("USING ERRCODE = '23514'")
        .map(|offset| cycle_error + offset)
        .expect("cycle guard must retain SQLSTATE 23514");

    assert!(
        dangling_guard < dangling_sqlstate
            && dangling_sqlstate < cycle_guard
            && cycle_guard < cycle_error
            && cycle_error < cycle_sqlstate,
        "dangling predecessor classification must remain before cycle classification"
    );
}
