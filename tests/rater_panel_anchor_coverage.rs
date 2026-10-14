//! Branch-coverage regression for distinct anchor insertion.

use psychometrics_commons_runtime::rater_panel::RaterPanelDefinition;

#[test]
fn draft_panel_accepts_distinct_anchor_references_in_order() {
    let mut panel = RaterPanelDefinition::new("panel", "revision", "design").expect("panel");

    panel
        .add_anchor_response("anchor_alpha")
        .expect("first anchor");
    panel
        .add_anchor_response("anchor_beta")
        .expect("distinct second anchor");

    assert_eq!(
        panel.anchor_response_refs(),
        ["anchor_alpha", "anchor_beta"]
    );
}
