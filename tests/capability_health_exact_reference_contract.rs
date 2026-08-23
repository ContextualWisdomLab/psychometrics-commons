//! Exact-spelling contracts for runtime capability-health identities.

use psychometrics_commons_runtime::health::{
    CapabilityHealth, CapabilityState, HealthContractError,
};

#[test]
fn capability_identity_rejects_padded_aliases() {
    for invalid in [
        " scoring",
        "scoring ",
        "\u{00a0}scoring",
        "scoring\u{2003}",
        "\u{202f}scoring",
        "scoring\u{3000}",
    ] {
        assert_eq!(
            CapabilityHealth::new(invalid, CapabilityState::Available, true),
            Err(HealthContractError::InvalidReference),
            "capability health must not canonicalize a caller-supplied identity alias"
        );
    }
}

#[test]
fn multilingual_capability_identity_is_preserved_exactly() {
    let capability = CapabilityHealth::new("채점_능력_α", CapabilityState::Available, true).unwrap();
    assert_eq!(capability.capability_ref(), "채점_능력_α");
}
