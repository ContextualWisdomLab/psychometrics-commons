//! Static contract for bounded supersession-lineage validation during migration reapply.
//!
//! Real `PostgreSQL` tests prove dangling and cyclic historical evidence is rejected.
//! This contract additionally prevents the reapply guard from regressing to a
//! per-row ancestor walk that materializes quadratic intermediate lineage on a
//! long correction chain. The migration should walk forward from root results,
//! visiting each reachable immutable result once, then reject any unvisited row.

const RESULT_SNAPSHOT_MIGRATION: &str = include_str!("../migrations/0007_result_snapshot.sql");

#[test]
fn supersession_reapply_walks_forward_from_roots_without_per_start_visited_arrays() {
    assert!(RESULT_SNAPSHOT_MIGRATION.contains("WITH RECURSIVE reachable_supersession_result AS"));
    assert!(RESULT_SNAPSHOT_MIGRATION.contains("WHERE root.supersedes_ref IS NULL"));
    assert!(RESULT_SNAPSHOT_MIGRATION
        .contains("successor.supersedes_ref = reachable.result_snapshot_ref"));
    assert!(
        RESULT_SNAPSHOT_MIGRATION.contains("LEFT JOIN reachable_supersession_result AS reachable")
    );
    assert!(RESULT_SNAPSHOT_MIGRATION.contains("WHERE reachable.result_snapshot_ref IS NULL"));
    assert!(
        !RESULT_SNAPSHOT_MIGRATION.contains("ARRAY[result_snapshot_ref]::text[] AS visited_refs")
    );
}
