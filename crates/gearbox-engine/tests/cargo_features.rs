//! `cargo_features` as a curation over a projected `[features]` table.
//!
//! The same split `config_schema.rs` asserts, one level over: Cargo states which
//! features exist and the description states which are worth offering and where
//! each belongs. What is proved here is the split and its two checks -- that a
//! curation is checked against the table rather than trusted, that absent and
//! empty are different answers, and that a feature scoped to a deployment kind
//! is refused for the kinds it excludes.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "clippy.toml's allow-unwrap-in-tests covers #[test] fns but not the helpers here"
)]

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use gearbox_engine::{SourceRoot, load_catalogue};
use gearbox_ir::{Catalogue, DiagnosticCode, GearId, Range, SourceId};

static NEXT: AtomicUsize = AtomicUsize::new(0);

const GEAR_RS: &str = r#"
#[toolkit::gear(name = "demo", capabilities = [system])]
pub struct DemoGear;
"#;

/// A crate declaring the shape the corpus actually has: Cargo's own `default`,
/// a test-only switch, and two real choices.
const MANIFEST: &str = "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n\
     [lib]\nname = \"demo\"\npath = \"src/lib.rs\"\n\n\
     [features]\ndefault = []\nintegration = []\notel = []\nk8s-auth = []\n";

/// The `gear.gdl` text for a description ending with `declared`.
fn source(declared: &str) -> String {
    format!(
        r#"
gear(
    maturity = "stable",
    name = "Demo",
    description = "d",
    category = "core-functionality",
    visibility = "internal",
    package = cargo(crate_name = "demo", lib = "demo", path = "."),
    {declared}
)
"#
    )
}

/// The zero-based line holding `needle`, for asserting a diagnostic's anchor.
fn line_of(text: &str, needle: &str) -> u32 {
    let found: Vec<u32> = text
        .lines()
        .enumerate()
        .filter(|(_, line)| line.contains(needle))
        .map(|(i, _)| u32::try_from(i).expect("fixtures are short"))
        .collect();
    assert_eq!(
        found.len(),
        1,
        "`{needle}` must appear on exactly one line of the fixture, found {found:?}"
    );
    found[0]
}

fn root(declared: &str) -> PathBuf {
    let nth = NEXT.fetch_add(1, Ordering::Relaxed);
    let root =
        std::env::temp_dir().join(format!("gbx-cargo-features-{}-{nth}", std::process::id()));
    drop(std::fs::remove_dir_all(&root));
    let crate_dir = root.join("demo");
    std::fs::create_dir_all(crate_dir.join("src")).unwrap();
    std::fs::write(crate_dir.join("gear.gdl"), source(declared)).unwrap();
    std::fs::write(crate_dir.join("Cargo.toml"), MANIFEST).unwrap();
    std::fs::write(crate_dir.join("src/lib.rs"), GEAR_RS).unwrap();
    root
}

fn catalogue(declared: &str) -> Catalogue {
    let path = root(declared);
    let source = SourceRoot::open(SourceId::new("demo").unwrap(), &path).unwrap();
    load_catalogue(&[source]).catalogue
}

fn demo(catalogue: &Catalogue) -> &gearbox_ir::GearDescriptor {
    catalogue
        .gears
        .get(&GearId::new("demo").unwrap())
        .expect("the demo gear is in the catalogue")
}

fn codes(catalogue: &Catalogue) -> Vec<DiagnosticCode> {
    catalogue.diagnostics.iter().map(|d| d.code).collect()
}

#[path = "support/resolve_fixtures.rs"]
mod support;

#[test]
fn the_description_curates_and_cargo_supplies_the_facts() {
    let catalogue = catalogue(
        r#"cargo_features = [feature("otel"), feature("k8s-auth", kinds = ["kubernetes"])],"#,
    );
    assert_eq!(codes(&catalogue), Vec::<DiagnosticCode>::new());

    let gear = demo(&catalogue);
    // Both halves survive, and they are not the same list. That is the whole
    // point: `default` and `integration` exist and are not offered.
    assert_eq!(
        gear.available_features.iter().cloned().collect::<Vec<_>>(),
        vec!["default", "integration", "k8s-auth", "otel"]
    );
    let curated = gear
        .cargo_features
        .as_ref()
        .expect("a curation was declared");
    assert_eq!(
        curated.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(),
        vec!["otel", "k8s-auth"],
        "declared order is kept: the list is an ordering as much as a filter"
    );
    assert!(curated[0].kinds.is_empty(), "no kinds means every kind");
    assert!(curated[1].kinds.contains("kubernetes"));
}

#[test]
fn a_curated_name_the_crate_does_not_declare_is_gbx0213() {
    let declared = r#"cargo_features = [feature("otel"), feature("otelll")],"#;
    let catalogue = catalogue(declared);
    assert_eq!(
        codes(&catalogue),
        vec![DiagnosticCode::ValidateFeatureUnknown]
    );

    // The good one still lands: one drifted name is not a reason to drop the
    // curation, and a panel showing nothing would be a worse answer than a
    // panel showing what still resolves.
    let curated = demo(&catalogue).cargo_features.as_ref().unwrap();
    assert_eq!(
        curated.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(),
        vec!["otel"]
    );

    // Anchored on the specific `feature("otelll")` call, not the file: each
    // `FeatureRecord` carries its own span, so a curation with several bad
    // names would not all collapse onto the same line.
    let diagnostic = catalogue.diagnostics.iter().next().expect("one diagnostic");
    let location = diagnostic
        .location
        .as_ref()
        .expect("the diagnostic carries a location");
    assert_ne!(
        location.range,
        Range::whole_file(),
        "must anchor on the feature(...) call, not the file: {diagnostic:#?}"
    );
    assert_eq!(
        location.range.start.line,
        line_of(&source(declared), r#"feature("otelll")"#),
        "must be anchored on the feature(\"otelll\") line"
    );
}

/// The distinction that made the field an `Option`.
#[test]
fn absent_and_empty_are_different_answers() {
    let uncurated = catalogue("");
    assert_eq!(
        demo(&uncurated).cargo_features,
        None,
        "nobody has curated this gear, so a client falls back to the projected table"
    );
    assert!(!demo(&uncurated).available_features.is_empty());

    let nothing_offered = catalogue("cargo_features = [],");
    assert_eq!(
        demo(&nothing_offered).cargo_features,
        Some(Vec::new()),
        "`types-registry` declares one feature and it gates Docker tests: \
         offering nothing is an answer, not a gap"
    );
}

/// A deployment kind that is not one of the three is a typo, and a typo that
/// survived would make the feature unofferable everywhere.
#[test]
fn an_unknown_deployment_kind_is_refused_at_evaluation() {
    let catalogue = catalogue(r#"cargo_features = [feature("otel", kinds = ["k8s"])],"#);
    assert!(
        codes(&catalogue).contains(&DiagnosticCode::GdlEval),
        "expected the evaluation to fail, got {:?}",
        codes(&catalogue)
    );
}

/// The check item 4 of the UX audit asked for, stated as a resolution.
///
/// `k8s-auth` compiles on any target and fails when it runs: it reads a
/// service-account token from a path that exists only in a pod. The gear is the
/// only thing that can know that, the profile is the only thing that knows
/// which deployment is being built, and neither alone is enough -- which is why
/// this is a resolve-time join rather than a catalogue check.
#[test]
fn a_feature_scoped_to_a_kind_is_refused_for_the_others() {
    use gearbox_ir::{CargoFeature, ProfileId};

    let mut catalogue = gearbox_ir::Catalogue::default();
    let mut gear = support::descriptor("demo");
    gear.cargo_features = Some(vec![
        CargoFeature {
            name: "otel".to_owned(),
            kinds: std::collections::BTreeSet::default(),
        },
        CargoFeature {
            name: "k8s-auth".to_owned(),
            kinds: ["kubernetes".to_owned()].into_iter().collect(),
        },
    ]);
    catalogue.gears.insert(GearId::new("demo").unwrap(), gear);

    let mut intent = support::intent(&["demo"]);
    intent.selected_gears[0].features = vec!["otel".to_owned(), "k8s-auth".to_owned()];

    let dev = ProfileId::new("dev").unwrap();
    let resolution = gearbox_engine::resolve::resolve(&catalogue, &intent, &dev);
    let reported: Vec<_> = resolution
        .diagnostics
        .iter()
        .filter(|d| d.code == DiagnosticCode::TopologyFeatureWrongKind)
        .collect();

    assert_eq!(
        reported.len(),
        1,
        "only the scoped feature is wrong: {reported:#?}"
    );
    assert!(
        reported[0].message.contains("k8s-auth"),
        "the message names the feature: {}",
        reported[0].message
    );
    assert!(
        reported[0].message.contains("embedded"),
        "and the kind the profile actually is: {}",
        reported[0].message
    );
}
