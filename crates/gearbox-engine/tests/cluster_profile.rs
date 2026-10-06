//! The cluster profile as a join key, on a fixture tree.
//!
//! A cluster profile is the routing key: the SDK maps it to
//! `ClientScope::new("cluster:{name}")` and resolves whatever backend is
//! registered there. Nothing checks it at compile time, so a name no
//! `impl ClusterProfile` supplies fails at *startup* with `ProfileNotBound`.
//! Treating it as a join key is what moves that failure to build time.
//!
//! The fixture reproduces `event-broker`'s real shape, which is the only
//! production cluster consumer in the platform: `deps = [cluster]`, a private
//! marker struct, and a `NAME` that is not the kebab-case of that struct's
//! identifier.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "clippy.toml's allow-unwrap-in-tests covers #[test] fns but not the helpers here"
)]

use std::path::{Path, PathBuf};

use gearbox_engine::{SourceRoot, load_catalogue};
use gearbox_ir::{Diagnostics, SourceId};

const GEAR_GDL: &str = r#"
gear(
    maturity = "stable",
    name = "Demo",
    description = "A cluster consumer.",
    category = "platform",
    visibility = "internal",
    package = cargo(crate_name = "demo", lib = "demo", path = "."),
    requires = [cluster.cache(profile = "event-broker")],
)
"#;

const GEAR_RS: &str = r#"
#[toolkit::gear(
    name = "demo",
    deps = [cluster],
    capabilities = [stateful],
    lifecycle(entry = "serve")
)]
pub struct DemoGear;
"#;

/// `event-broker`'s real marker: private, and `NAME` != kebab of the ident.
const PROFILE_RS: &str = r#"
#[derive(Debug, Clone, Copy)]
struct EventBrokerProfile;

impl ClusterProfile for EventBrokerProfile {
    const NAME: &'static str = "event-broker";
}
"#;

/// Build the fixture under a deterministic directory, replacing any previous run.
fn fixture(slug: &str, profile: Option<&str>) -> PathBuf {
    let root = std::env::temp_dir().join(format!("gbx-cluster-profile-{slug}"));
    // Absent on the first run; a stale tree from a previous run must not leak in.
    drop(std::fs::remove_dir_all(&root));
    let crate_dir = root.join("demo");
    std::fs::create_dir_all(crate_dir.join("src")).unwrap();
    std::fs::write(crate_dir.join("gear.gdl"), GEAR_GDL).unwrap();
    std::fs::write(crate_dir.join("src/lib.rs"), GEAR_RS).unwrap();
    if let Some(text) = profile {
        std::fs::write(crate_dir.join("src/profile.rs"), text).unwrap();
    }
    root
}

fn diagnostics(root: &Path) -> Diagnostics {
    let source = SourceRoot::open(SourceId::new("fixture").unwrap(), root).unwrap();
    load_catalogue(&[source]).catalogue.diagnostics
}

fn codes(diagnostics: &Diagnostics) -> Vec<String> {
    let mut v: Vec<String> = diagnostics
        .iter()
        .map(|d| d.code.as_str().to_owned())
        .collect();
    v.sort();
    v.dedup();
    v
}

#[test]
fn a_profile_no_impl_supplies_is_an_error() {
    let root = fixture("missing", None);
    let diagnostics = diagnostics(&root);
    assert!(
        codes(&diagnostics).contains(&"GBX0508".to_owned()),
        "a requirement naming an unimplemented profile can never be bound; got {:?}",
        codes(&diagnostics)
    );
    let message = diagnostics
        .iter()
        .find(|d| d.code.as_str() == "GBX0508")
        .map(|d| d.message.clone())
        .unwrap();
    assert!(
        message.contains("implements no `ClusterProfile` at all"),
        "the message must distinguish 'none at all' from 'not this one': {message}"
    );
}

#[test]
fn the_real_marker_shape_satisfies_the_join() {
    let root = fixture("present", Some(PROFILE_RS));
    let codes = codes(&diagnostics(&root));
    assert!(
        !codes.contains(&"GBX0508".to_owned()),
        "a private marker whose NAME matches must satisfy the join; got {codes:?}"
    );
}

#[test]
fn the_name_is_read_from_the_const_not_derived_from_the_ident() {
    // Kebab-casing `EventBrokerProfile` would give `event-broker-profile`. If the
    // projector derived the name, this fixture -- whose NAME says `evbk` -- would
    // still appear to satisfy a requirement for `event-broker`. It must not.
    let renamed = PROFILE_RS.replace(r#""event-broker""#, r#""evbk""#);
    let root = fixture("renamed", Some(&renamed));
    let diagnostics = diagnostics(&root);

    assert!(
        codes(&diagnostics).contains(&"GBX0508".to_owned()),
        "renaming NAME must break the join"
    );
    let message = diagnostics
        .iter()
        .find(|d| d.code.as_str() == "GBX0508")
        .map(|d| d.message.clone())
        .unwrap();
    assert!(
        message.contains("evbk"),
        "the message must list the projected name, proving NAME was read: {message}"
    );
    assert!(
        !message.contains("event-broker-profile"),
        "nothing may derive a profile name from the marker's identifier: {message}"
    );
}

#[test]
fn a_cluster_requirement_records_the_colocation_constraint() {
    let root = fixture("colocation", Some(PROFILE_RS));
    let diagnostics = diagnostics(&root);
    let hint = diagnostics
        .iter()
        .find(|d| d.code.as_str() == "GBX0607")
        .expect("a cluster consumer must be told the edge is not severable");

    assert_eq!(hint.severity, gearbox_ir::Severity::Hint, "not a complaint");
    assert!(
        hint.evidence.is_some(),
        "GBX0607 asserts a runtime limitation, so it must cite one \
         (cpt-gearbox-nfr-evidence-cited)"
    );
    assert!(
        hint.help.as_deref().is_some_and(|h| h.contains("keep")),
        "`deps = [cluster]` is correct today; the hint must not suggest removing it"
    );
}

#[test]
fn the_hint_is_absent_without_a_cluster_dependency() {
    // A gear that requires nothing from cluster gets no hint, and a gear that
    // requires something but does not depend on cluster is a different problem
    // than the one GBX0607 describes.
    let root = fixture("nodeps", Some(PROFILE_RS));
    let crate_dir = root.join("demo");
    std::fs::write(
        crate_dir.join("src/lib.rs"),
        GEAR_RS.replace("deps = [cluster],\n", ""),
    )
    .unwrap();

    let codes = codes(&diagnostics(&root));
    assert!(
        !codes.contains(&"GBX0607".to_owned()),
        "the hint is about a co-location edge that exists; got {codes:?}"
    );
}
