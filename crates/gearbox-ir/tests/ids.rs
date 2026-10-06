//! Identifier validation.
//!
//! The `GearId` cases are not invented: they are lifted from the
//! `#[toolkit::gear]` macro's own UI test suite in `gears-rust`
//! (`libs/toolkit-macros-tests/tests/ui/{pass,fail}/gear_name_*.rs`) plus every
//! gear name actually declared in that repository. Gearbox must accept exactly
//! what the macro accepts: accepting more would let `gearbox validate` pass a
//! product that cannot compile, and accepting less would reject a real gear.

use gearbox_ir::{
    ApplicationId, CapabilityId, ContractId, GearId, NodeId, ProfileId, ProviderId, RelPath,
    RequirementId, SourceId,
};

/// Every gear name declared in `gears-rust` today, plus the macro's own
/// `ui/pass/gear_name_kebab_case.rs` fixtures (`gear-v2`, `system`).
const REAL_GEAR_NAMES: &[&str] = &[
    "account-management",
    "api-contracts",
    "api-contracts-consumer",
    "api-gateway",
    "authn-resolver",
    "authz-resolver",
    "bss-ledger",
    "bss-pricing",
    "bss-rate-provider",
    "bss-rate-provider-ecb-plugin",
    "bss-rate-provider-http-json-plugin",
    "calculator",
    "calculator-gateway",
    "chat-engine",
    "cluster",
    "credstore",
    "event-broker",
    "file-parser",
    "service-discovery",
    "gear-v2",
    "grpc-hub",
    "keycloak-idp-plugin",
    "mini-chat",
    "nodes-registry",
    "noop-usage-collector-plugin",
    "oagw",
    "oidc-authn-plugin",
    "payments-audit",
    "resource-group",
    "rg-tr-plugin",
    "simple-user-settings",
    "single-tenant-tr-plugin",
    "static-authn-plugin",
    "static-authz-plugin",
    "static-credstore-plugin",
    "static-idp-plugin",
    "static-mini-chat-audit-plugin",
    "static-mini-chat-model-policy-plugin",
    "static-tr-plugin",
    "system",
    "tenant-resolver",
    "timescaledb-usage-collector-plugin",
    "tr-authz-plugin",
    "types-registry",
    "usage-collector",
    "users-info",
];

/// The macro's `ui/fail/gear_name_*.rs` fixtures, each with the rule it breaks.
const MACRO_REJECTED_NAMES: &[(&str, &str)] = &[
    ("-parser", "gear_name_starts_hyphen"),
    ("FileParser", "gear_name_uppercase"),
    ("file--parser", "gear_name_consecutive_hyphens"),
    ("file_parser", "gear_name_snake_case"),
    ("parser-", "gear_name_ends_hyphen"),
];

#[test]
fn accepts_every_real_gear_name() {
    for name in REAL_GEAR_NAMES {
        assert!(
            GearId::new(*name).is_ok(),
            "GearId rejected `{name}`, which is a real gear name in gears-rust"
        );
    }
}

#[test]
fn rejects_exactly_what_the_gear_macro_rejects() {
    for (name, fixture) in MACRO_REJECTED_NAMES {
        assert!(
            GearId::new(*name).is_err(),
            "GearId accepted `{name}`, but #[toolkit::gear] rejects it \
             (see gears-rust ui/fail/{fixture}.rs)"
        );
    }
}

#[test]
fn rejects_other_malformed_gear_names() {
    for bad in [
        "",            // empty
        "1st-gear",    // must start with a letter
        "gear name",   // no spaces
        "gear.name",   // no dots
        "gear/name",   // no slashes
        "Gear",        // no uppercase
        "gear-",       // trailing hyphen
        "-gear",       // leading hyphen
        "gear--x",     // consecutive hyphens
        "gear_x",      // no underscores
        "ge\u{00e4}r", // ASCII only
    ] {
        assert!(GearId::new(bad).is_err(), "GearId accepted `{bad}`");
    }
}

#[test]
fn gear_ids_order_lexicographically() {
    // The resolver relies on this ordering for byte-identical output.
    let mut ids: Vec<GearId> = ["types-registry", "api-gateway", "cluster"]
        .iter()
        .map(|s| GearId::new(*s).unwrap())
        .collect();
    ids.sort();
    let ordered: Vec<&str> = ids.iter().map(GearId::as_str).collect();
    assert_eq!(ordered, ["api-gateway", "cluster", "types-registry"]);
}

#[test]
fn kebab_rule_is_shared_by_all_kebab_ids() {
    for good in ["api-gateway", "dev", "prod", "gears-rust"] {
        assert!(ProfileId::new(good).is_ok(), "ProfileId rejected `{good}`");
        assert!(SourceId::new(good).is_ok(), "SourceId rejected `{good}`");
        assert!(
            ApplicationId::new(good).is_ok(),
            "ApplicationId rejected `{good}`"
        );
    }
    for bad in ["Dev", "dev_1", "dev-"] {
        assert!(ProfileId::new(bad).is_err(), "ProfileId accepted `{bad}`");
        assert!(SourceId::new(bad).is_err(), "SourceId accepted `{bad}`");
        assert!(
            ApplicationId::new(bad).is_err(),
            "ApplicationId accepted `{bad}`"
        );
    }
}

#[test]
fn contract_ids_carry_gear_base_name_and_major() {
    for good in [
        "api-contracts/PaymentApi@v1",
        "api-contracts/PaymentApi@v2",
        "payments-audit/PaymentsAuditApi@v1",
        "types-registry/TypesRegistryBackend@v10",
    ] {
        assert!(
            ContractId::new(good).is_ok(),
            "ContractId rejected `{good}`"
        );
    }

    for bad in [
        "PaymentApi@v1",                // missing gear
        "api-contracts/PaymentApi",     // missing version
        "api-contracts/PaymentApi@1",   // missing the `v`
        "api-contracts/paymentApi@v1",  // trait names are PascalCase
        "api-contracts/PaymentApi@v01", // no leading zero
        "api-contracts/PaymentApi@v",   // no major
        "API-Contracts/PaymentApi@v1",  // gear must be kebab
        "api-contracts/Payment_Api@v1", // no underscores in a trait name
    ] {
        assert!(ContractId::new(bad).is_err(), "ContractId accepted `{bad}`");
    }
}

#[test]
fn requirement_ids_carry_gear_namespace_and_ordinal() {
    for good in [
        "payments-audit#cluster.cache[0]",
        "payments-audit#cluster.leader-election[1]",
        "mini-chat#contract.consumes[12]",
    ] {
        assert!(RequirementId::new(good).is_ok(), "rejected `{good}`");
    }
    for bad in [
        "payments-audit#cluster.cache",     // missing ordinal
        "payments-audit#cluster.cache[]",   // empty ordinal
        "payments-audit#cache[0]",          // namespace needs >= 2 segments
        "payments-audit#cluster.cache[00]", // no leading zero
        "cluster.cache[0]",                 // missing gear
    ] {
        assert!(RequirementId::new(bad).is_err(), "accepted `{bad}`");
    }
}

#[test]
fn capability_ids_are_dotted_kebab() {
    for good in [
        "cluster.cache.linearizable",
        "cluster.cache.prefix-watch",
        "runtime.rest-host",
    ] {
        assert!(CapabilityId::new(good).is_ok(), "rejected `{good}`");
    }
    for bad in [
        "linearizable",
        "cluster..cache",
        "cluster.Cache",
        "cluster.cache.",
    ] {
        assert!(CapabilityId::new(bad).is_err(), "accepted `{bad}`");
    }
}

#[test]
fn provider_and_node_ids_are_two_part() {
    assert!(ProviderId::new("cache:postgres").is_ok());
    assert!(ProviderId::new("leader-election:standalone").is_ok());
    assert!(ProviderId::new("postgres").is_err());
    assert!(ProviderId::new("cache:").is_err());

    // Node payloads are deliberately opaque: they embed contract ids and arrows.
    assert!(NodeId::new("binding:payments-audit->api-contracts/PaymentApi@v1").is_ok());
    assert!(NodeId::new("decision:cut:payments-audit->api-contracts").is_ok());
    assert!(NodeId::new("gear:payments-audit").is_ok());
    assert!(NodeId::new("no-colon").is_err());
}

#[test]
fn rel_paths_are_relative_normalized_and_contained() {
    let p = RelPath::new("gears/system/cluster/cluster").unwrap();
    assert_eq!(p.as_str(), "gears/system/cluster/cluster");

    // Redundant separators and `.` segments are dropped, so two spellings of
    // one path hash identically.
    assert_eq!(
        RelPath::new("gears//system/./cluster").unwrap().as_str(),
        "gears/system/cluster"
    );

    assert!(RelPath::here().is_here());
    assert_eq!(RelPath::new("./").unwrap().as_str(), ".");

    for bad in [
        "/abs/path",
        "../escape",
        "gears/../../escape",
        "a\\b",
        "",
        "C:",
        "C:/windows",
        "C:foo",
        "gears/C:escape",
        "NUL",
        "CON",
        "COM1",
        "aux.txt",
        "out/NUL",
        "gears/\0hidden",
        "..\0",
    ] {
        assert!(RelPath::new(bad).is_err(), "RelPath accepted `{bad}`");
    }
    assert!(
        RelPath::here().resolve("C:/abs").is_err(),
        "resolve must refuse a Windows drive"
    );
    assert!(
        RelPath::here().resolve("..\0").is_err(),
        "resolve must refuse a control character even inside `..`"
    );
    assert!(
        RelPath::new("gears").unwrap().resolve("NUL").is_err(),
        "resolve must refuse a Windows device name"
    );
}

#[test]
fn rel_path_resolve_walks_up_but_cannot_escape_the_root() {
    // `gear.gdl` legitimately points at a sibling crate, so upward traversal
    // must work at resolution time even though a stored RelPath forbids `..`.
    let gear_dir = RelPath::new("examples/toolkit/api-contracts/api-contracts").unwrap();
    assert_eq!(
        gear_dir.resolve("../api-contracts-sdk").unwrap().as_str(),
        "examples/toolkit/api-contracts/api-contracts-sdk"
    );
    assert_eq!(gear_dir.resolve(".").unwrap(), gear_dir);

    let deep = RelPath::new("gears/payments-audit/payments-audit").unwrap();
    assert_eq!(
        deep.resolve("../../../examples/toolkit/api-contracts/api-contracts-sdk")
            .unwrap()
            .as_str(),
        "examples/toolkit/api-contracts/api-contracts-sdk"
    );

    // Escaping above the source root is what we must never allow.
    assert!(
        RelPath::new("gears")
            .unwrap()
            .resolve("../../outside")
            .is_err()
    );
}

#[test]
fn rel_path_join_and_parent() {
    let root = RelPath::new("gears/system").unwrap();
    let leaf = RelPath::new("cluster/cluster").unwrap();
    assert_eq!(
        root.join(&leaf).unwrap().as_str(),
        "gears/system/cluster/cluster"
    );
    assert_eq!(root.join(&RelPath::here()).unwrap(), root);
    assert_eq!(RelPath::here().join(&leaf).unwrap(), leaf);
    assert_eq!(root.parent().as_str(), "gears");
    assert_eq!(RelPath::new("gears").unwrap().parent(), RelPath::here());
}

#[test]
fn ids_round_trip_through_json_as_plain_strings() {
    let id = GearId::new("payments-audit").unwrap();
    let json = serde_json::to_string(&id).unwrap();
    assert_eq!(
        json, "\"payments-audit\"",
        "ids must serialize as bare strings"
    );
    assert_eq!(serde_json::from_str::<GearId>(&json).unwrap(), id);

    let path = RelPath::new("gears/payments-audit").unwrap();
    assert_eq!(
        serde_json::to_string(&path).unwrap(),
        "\"gears/payments-audit\""
    );
}

#[test]
fn deserialization_enforces_the_same_rules_as_construction() {
    // An id read from a hand-edited product.lock must be validated exactly as
    // one built in memory, or the lock becomes a way to smuggle in bad state.
    assert!(serde_json::from_str::<GearId>("\"Bad_Name\"").is_err());
    assert!(serde_json::from_str::<ContractId>("\"nope\"").is_err());
    assert!(serde_json::from_str::<RelPath>("\"../escape\"").is_err());
}
