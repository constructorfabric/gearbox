//! The closed sets GDL exposes, and their mapping back to the typed model.
//!
//! Each namespace here mirrors an enum in `gearbox_ir`. The mapping is written
//! out explicitly rather than derived from a string, so that adding a variant
//! to the IR without exposing it here is a compile error in the `From` arm
//! rather than a silently missing piece of vocabulary.

use gearbox_ir::intent::BindingMode;
use gearbox_ir::{ClusterPrimitive, ContractKind, RuntimeCap, Transport};

use crate::values::{GdlEnum, GdlNamespace};

/// `cap.*` -- the seven runtime capabilities `#[toolkit::gear]` accepts.
pub const CAP: GdlNamespace = GdlNamespace::new(
    "cap",
    &[
        "db",
        "rest",
        "rest_host",
        "stateful",
        "system",
        "grpc_hub",
        "grpc",
    ],
);

/// `transport.*` -- the transports the runtime's client wiring implements.
pub const TRANSPORT: GdlNamespace = GdlNamespace::new("transport", &["local", "rest", "grpc"]);

/// `contract_kind.*` -- the four trait-name suffixes.
pub const CONTRACT_KIND: GdlNamespace = GdlNamespace::new(
    "contract_kind",
    &["api", "embedded", "backend", "extension"],
);

/// `cluster_cap.*` -- the cache capabilities the cluster SDK declares.
pub const CLUSTER_CAP: GdlNamespace =
    GdlNamespace::new("cluster_cap", &["linearizable", "watch", "prefix_watch"]);

/// The gear categories the platform actually uses.
///
/// Taken from the `gear.toml` files the platform team committed, one per gear,
/// before they were retired into `gear.gdl` (ADR-0002 amendment, 2026-10-02):
/// those seven values and no others. Not a `GdlNamespace` like the closed sets
/// above, because `category` is a plain string in the description and this list
/// drives a **warning**, not a refusal -- the taxonomy is visibly still
/// settling, with `cluster` filed under `serverless` and `account-management`
/// under `oss`.
///
/// `example` is Gearbox's own addition. No `gear.toml` ever existed under
/// `examples/`, so the platform's list had no slot for an example gear; if the
/// team adds one, rename to match rather than keeping both.
pub const KNOWN_CATEGORIES: &[&str] = &[
    "api-ingress",
    "bss",
    "core-functionality",
    "core-platform-integration",
    "example",
    "gen-ai",
    "oss",
    "serverless",
];

/// `binding_mode.*` -- what an operator may ask for regarding one edge.
pub const BINDING_MODE: GdlNamespace =
    GdlNamespace::new("binding_mode", &["auto", "local", "remote"]);

/// Resolve a `cap.*` member to its typed capability.
///
/// # Errors
/// Returns the offending value's `Display` form when it is not a `cap` member.
pub fn runtime_cap(value: &GdlEnum) -> Result<RuntimeCap, String> {
    let variant = value
        .variant_in("cap")
        .ok_or_else(|| format!("expected a `cap.*` capability, got `{value}`"))?;
    // Reuses the IR's own parser, which spells the seven exactly as the gear
    // macro does, so the two cannot drift.
    RuntimeCap::parse(variant).ok_or_else(|| format!("unknown capability `{variant}`"))
}

/// Resolve a `transport.*` member.
///
/// # Errors
/// Returns the offending value's `Display` form when it is not a `transport` member.
pub fn transport(value: &GdlEnum) -> Result<Transport, String> {
    let variant = value
        .variant_in("transport")
        .ok_or_else(|| format!("expected a `transport.*` value, got `{value}`"))?;
    match variant {
        "local" => Ok(Transport::Local),
        "rest" => Ok(Transport::Rest),
        "grpc" => Ok(Transport::Grpc),
        other => Err(format!("unknown transport `{other}`")),
    }
}

/// Resolve a `contract_kind.*` member.
///
/// # Errors
/// Returns the offending value's `Display` form when it is not a `contract_kind` member.
pub fn contract_kind(value: &GdlEnum) -> Result<ContractKind, String> {
    let variant = value
        .variant_in("contract_kind")
        .ok_or_else(|| format!("expected a `contract_kind.*` value, got `{value}`"))?;
    match variant {
        "api" => Ok(ContractKind::Api),
        "embedded" => Ok(ContractKind::Embedded),
        "backend" => Ok(ContractKind::Backend),
        "extension" => Ok(ContractKind::Extension),
        other => Err(format!("unknown contract kind `{other}`")),
    }
}

/// Resolve a `binding_mode.*` member.
///
/// # Errors
/// Returns the offending value's `Display` form when it is not a `binding_mode` member.
pub fn binding_mode(value: &GdlEnum) -> Result<BindingMode, String> {
    let variant = value
        .variant_in("binding_mode")
        .ok_or_else(|| format!("expected a `binding_mode.*` value, got `{value}`"))?;
    match variant {
        "auto" => Ok(BindingMode::Auto),
        "local" => Ok(BindingMode::Local),
        "remote" => Ok(BindingMode::Remote),
        other => Err(format!("unknown binding mode `{other}`")),
    }
}

/// Resolve a `cluster_cap.*` member to the capability id the IR uses.
///
/// The returned strings are the constants in `gearbox_ir::capabilities`, not
/// re-spelled literals, so a rename there reaches GDL.
///
/// # Errors
/// Returns the offending value's `Display` form when it is not a `cluster_cap`
/// member, or when the member does not apply to `primitive`.
pub fn cluster_capability(
    value: &GdlEnum,
    primitive: ClusterPrimitive,
) -> Result<&'static str, String> {
    use gearbox_ir::capabilities as caps;

    let variant = value
        .variant_in("cluster_cap")
        .ok_or_else(|| format!("expected a `cluster_cap.*` value, got `{value}`"))?;

    let id = match (primitive, variant) {
        (ClusterPrimitive::Cache, "linearizable") => caps::CACHE_LINEARIZABLE,
        (ClusterPrimitive::Cache, "watch") => caps::CACHE_WATCH,
        (ClusterPrimitive::Cache, "prefix_watch") => caps::CACHE_PREFIX_WATCH,
        (ClusterPrimitive::LeaderElection, "linearizable") => caps::LEADER_ELECTION_LINEARIZABLE,
        (ClusterPrimitive::Lock, "linearizable") => caps::LOCK_LINEARIZABLE,
        // `prefix_watch` is a cache property; asking for it on a lock or an
        // election is a description of something that does not exist, so it is
        // refused rather than ignored.
        (p, v) => {
            return Err(format!(
                "`cluster_cap.{v}` does not apply to `cluster.{}`; applicable: {}",
                p.slug(),
                p.nameable_capabilities().join(", ")
            ));
        }
    };
    Ok(id)
}

/// Every namespace, for registration into the globals.
pub const ALL_NAMESPACES: &[(&str, GdlNamespace)] = &[
    ("cap", CAP),
    ("transport", TRANSPORT),
    ("contract_kind", CONTRACT_KIND),
    ("cluster_cap", CLUSTER_CAP),
    ("binding_mode", BINDING_MODE),
];
