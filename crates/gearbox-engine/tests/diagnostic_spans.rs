//! Where a diagnostic points, asserted from a real description.
//!
//! **Not from a literal `ProductIntent`, and that is the whole reason this file
//! exists separately.** Every fixture in `tests/support/resolve_fixtures.rs`
//! sets `declared_at: None`, so a test built that way exercises the *fallback*
//! path -- it would pass unchanged if every span in the engine were deleted. A
//! span only exists if the description text was evaluated, so these tests
//! evaluate text.
//!
//! **No corpus, deliberately.** The sibling real-tree tests skip when
//! `../gears-rust` is absent, and a skipping test proves nothing. Every product
//! here names gears no catalogue has, which is exactly what makes the
//! unknown-gear checks fire against an empty `Catalogue`.
//!
//! Each assertion is the *exact* line the declaration is written on, found from
//! the fixture text rather than hard-coded, so editing a fixture cannot quietly
//! turn a real span into a passing `> 0`.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "clippy.toml's allow-unwrap-in-tests covers #[test] fns but not the helpers here; \
              a fixture builder that propagates errors instead of panicking obscures the \
              assertion it exists to support"
)]

use std::path::Path;

use gearbox_engine::product::eval_product_text;
use gearbox_engine::resolve::resolve_at;
use gearbox_ir::{Catalogue, Diagnostic, ProductIntent, ProfileId, Range};

/// Where the fixture pretends to live, so the URI is absolute and openable.
const PRODUCT_PATH: &str = "/gbx-spans/product.gdl";

/// The zero-based line holding `needle`. Panics rather than returning an
/// `Option`: a fixture that no longer contains what a test anchors on is a
/// broken test, not a skipped one.
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

fn intent(text: &str) -> ProductIntent {
    let scan = eval_product_text(Path::new(PRODUCT_PATH), None, text);
    scan.intent
        .unwrap_or_else(|| panic!("fixture must evaluate: {:#?}", scan.diagnostics))
}

/// Resolve against an empty catalogue, which is what makes every named gear
/// unknown.
fn diagnostics_of(text: &str, profile: &str) -> Vec<Diagnostic> {
    let intent = intent(text);
    let resolution = resolve_at(
        &Catalogue::default(),
        &intent,
        &ProfileId::new(profile).expect("kebab profile id"),
        Some(Path::new(PRODUCT_PATH)),
    );
    resolution.diagnostics.iter().cloned().collect()
}

fn find<'a>(diagnostics: &'a [Diagnostic], needle: &str) -> &'a Diagnostic {
    diagnostics
        .iter()
        .find(|d| d.message.contains(needle))
        .unwrap_or_else(|| {
            panic!(
                "no diagnostic mentioning `{needle}`; got {:#?}",
                diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>()
            )
        })
}

/// Assert the diagnostic is anchored on the line `needle` is written on.
fn assert_anchored(diagnostic: &Diagnostic, text: &str, needle: &str) {
    let location = diagnostic
        .location
        .as_ref()
        .unwrap_or_else(|| panic!("`{}` carries no location", diagnostic.message));
    assert!(
        location.uri.ends_with(PRODUCT_PATH),
        "must point at the description, not `{}`",
        location.uri
    );
    assert_ne!(
        location.range,
        Range::whole_file(),
        "the whole-file sentinel is not published to an editor at all: {diagnostic:#?}"
    );
    assert_eq!(
        location.range.start.line,
        line_of(text, needle),
        "`{}` must be anchored on the line holding `{needle}`",
        diagnostic.message
    );
}

const UNKNOWN_GEAR: &str = r#"product(
    id = "spans-probe",
    name = "Spans Probe",
    version = "0.1.0",
    sources = [source(id = "somewhere", at = path("."))],
    profiles = [embedded(id = "dev")],
    default_profile = "dev",
    gears = [
        use_gear("not-in-any-catalogue", source = "somewhere"),
    ],
)
"#;

/// `use_gear("x")` naming nothing points at that `use_gear`, not at line 1.
///
/// The case the CLI used to render as `product.gdl:1` for every unknown gear in
/// a description, however far down the file it was written.
#[test]
fn an_unknown_gear_is_anchored_on_its_use_gear() {
    let diagnostics = diagnostics_of(UNKNOWN_GEAR, "dev");
    let diagnostic = find(&diagnostics, "not-in-any-catalogue");
    assert_anchored(
        diagnostic,
        UNKNOWN_GEAR,
        "use_gear(\"not-in-any-catalogue\"",
    );
}

const UNHONOURED_ENDPOINT: &str = r#"product(
    id = "spans-probe",
    name = "Spans Probe",
    version = "0.1.0",
    sources = [source(id = "somewhere", at = path("."))],
    profiles = [embedded(id = "dev")],
    default_profile = "dev",
    gears = [
        use_gear("consumer", source = "somewhere"),
    ],
    bindings = [
        bind(
            consumer = "consumer",
            contract = "provider/Api@v1",
            mode = binding_mode.remote,
            transport = transport.rest,
            endpoint = "http://elsewhere",
        ),
    ],
)
"#;

/// `bind(endpoint = ...)` is refused at the `bind(...)` that wrote it.
#[test]
fn an_unhonoured_endpoint_is_anchored_on_its_bind() {
    let diagnostics = diagnostics_of(UNHONOURED_ENDPOINT, "dev");
    let diagnostic = find(&diagnostics, "endpoint");
    assert_anchored(diagnostic, UNHONOURED_ENDPOINT, "bind(");
}

/// A declaration with no recorded span still gets an answer, and it is the file.
///
/// The fallback is permanent, not a gap: a product built in a test carries no
/// spans at all, and `spans::declared_or_file` is what keeps those callers
/// working. Pinned so the fallback is not "simplified" away once most
/// declarations have spans.
#[test]
fn a_declaration_with_no_span_falls_back_to_the_file() {
    let mut intent = intent(UNKNOWN_GEAR);
    for selection in &mut intent.selected_gears {
        selection.declared_at = None;
    }
    let resolution = resolve_at(
        &Catalogue::default(),
        &intent,
        &ProfileId::new("dev").unwrap(),
        Some(Path::new(PRODUCT_PATH)),
    );
    let diagnostics: Vec<Diagnostic> = resolution.diagnostics.iter().cloned().collect();
    let location = find(&diagnostics, "not-in-any-catalogue")
        .location
        .as_ref()
        .expect("the fallback is a location, not the absence of one");
    assert!(location.uri.ends_with(PRODUCT_PATH), "{}", location.uri);
    assert_eq!(location.range, Range::whole_file());
}

// --------------------------------------------------------------------------
// The GDL layer. These fire during evaluation, so they need no catalogue and
// no resolution -- `eval_product_text` alone produces them.

/// Everything wrong with `text`, straight from the evaluator.
///
/// Separate from `diagnostics_of`: a description this broken has no intent to
/// resolve, and `intent()` would panic before the assertion ran.
fn eval_diagnostics(text: &str) -> Vec<Diagnostic> {
    eval_product_text(Path::new(PRODUCT_PATH), None, text)
        .diagnostics
        .iter()
        .cloned()
        .collect()
}

const DUPLICATE_SOURCE: &str = r#"product(
    id = "spans-probe",
    name = "Spans Probe",
    version = "0.1.0",
    sources = [
        source(id = "twice", at = path(".")),
        source(id = "twice", at = path("./other")),
    ],
    profiles = [embedded(id = "dev")],
    default_profile = "dev",
    gears = [],
)
"#;

/// A source declared twice points at the second `source(...)`, not at line 1.
#[test]
fn a_duplicate_source_is_anchored_on_its_declaration() {
    let diagnostics = eval_diagnostics(DUPLICATE_SOURCE);
    let diagnostic = find(&diagnostics, "is declared twice");
    assert_anchored(
        diagnostic,
        DUPLICATE_SOURCE,
        r#"source(id = "twice", at = path("./other"))"#,
    );
}

const CLUSTER_SCOPE_BOUND_TWICE: &str = r#"product(
    id = "spans-probe",
    name = "Spans Probe",
    version = "0.1.0",
    sources = [source(id = "somewhere", at = path("."))],
    profiles = [embedded(id = "dev")],
    default_profile = "dev",
    gears = [],
    cluster_profiles = [
        cluster_profile(name = "cache", cache = provider("standalone"), profiles = ["dev"]),
        cluster_profile(name = "cache", cache = provider("postgres"), profiles = ["dev"]),
    ],
)
"#;

/// A cluster scope bound twice in one profile points at the second
/// `cluster_profile(...)`, not at line 1.
#[test]
fn a_cluster_scope_bound_twice_is_anchored_on_its_second_declaration() {
    let diagnostics = eval_diagnostics(CLUSTER_SCOPE_BOUND_TWICE);
    let diagnostic = find(&diagnostics, "is bound twice");
    assert_anchored(
        diagnostic,
        CLUSTER_SCOPE_BOUND_TWICE,
        r#"cluster_profile(name = "cache", cache = provider("postgres")"#,
    );
}

const BAD_GEAR_ID: &str = r#"product(
    id = "spans-probe",
    name = "Spans Probe",
    version = "0.1.0",
    sources = [source(id = "somewhere", at = path("."))],
    profiles = [embedded(id = "dev")],
    default_profile = "dev",
    gears = [
        use_gear("Not Kebab", source = "somewhere"),
    ],
)
"#;

/// An id that is not kebab-case points at the `use_gear(...)` that wrote it.
///
/// Reaches the anchor through `gear_id`, which takes the span from its caller --
/// the case that would otherwise stay file-level because the validator is handed
/// a bare string.
#[test]
fn an_invalid_gear_id_is_anchored_on_its_use_gear() {
    let diagnostics = eval_diagnostics(BAD_GEAR_ID);
    let diagnostic = find(&diagnostics, "not a valid gear id");
    assert_anchored(diagnostic, BAD_GEAR_ID, r#"use_gear("Not Kebab""#);
}

const BAD_DISCOVERY: &str = r#"product(
    id = "spans-probe",
    name = "Spans Probe",
    version = "0.1.0",
    sources = [source(id = "somewhere", at = path("."))],
    profiles = [
        kubernetes(id = "k8s", discovery = "elsewhere"),
    ],
    default_profile = "k8s",
    gears = [],
)
"#;

/// An unknown `discovery` points at the profile that asked for it.
#[test]
fn an_unknown_discovery_is_anchored_on_its_profile() {
    let diagnostics = eval_diagnostics(BAD_DISCOVERY);
    let diagnostic = find(&diagnostics, "asks for discovery");
    assert_anchored(diagnostic, BAD_DISCOVERY, r#"kubernetes(id = "k8s""#);
}

const BAD_TEMPLATES: &str = r#"product(
    id = "spans-probe",
    name = "Spans Probe",
    version = "0.1.0",
    sources = [source(id = "somewhere", at = path("."))],
    profiles = [embedded(id = "dev")],
    default_profile = "dev",
    gears = [],
    templates = git(url = "https://example.invalid", tag = "v1"),
)
"#;

/// `templates` stays file-level, and that is the boundary worth pinning.
///
/// The `templates` record takes no `eval`, so no span exists for it -- the point
/// of the explicit `Location::file` at that call site. If somebody later gives
/// that record a span, this test is what says the fallback stopped being
/// necessary, rather than the change going unnoticed.
#[test]
fn a_declaration_that_records_no_span_stays_file_level() {
    let diagnostics = eval_diagnostics(BAD_TEMPLATES);
    let diagnostic = find(&diagnostics, "templates");
    let location = diagnostic
        .location
        .as_ref()
        .expect("still a location, just not a position");
    assert_eq!(
        location.range,
        Range::whole_file(),
        "no span is recorded for `templates`, so the honest answer is the file"
    );
}

const PINNED_UNDER_EMBEDDED: &str = r#"product(
    id = "spans-probe",
    name = "Spans Probe",
    version = "0.1.0",
    sources = [source(id = "somewhere", at = path("."))],
    profiles = [embedded(id = "dev")],
    default_profile = "dev",
    gears = [
        use_gear("anchor-gear", source = "somewhere"),
    ],
    applications = [
        application("second", anchor = "anchor-gear"),
    ],
)
"#;

/// An `application(...)` an embedded profile cannot honour points at itself.
///
/// The row Part 1 deferred: the complaint is a conflict between the pin and the
/// profile, and the pin is the declaration that *asks* for something, so it is
/// the one underlined. It needed `ApplicationPin` to carry a span, which is what
/// this part added.
#[test]
fn an_application_an_embedded_profile_cannot_honour_is_anchored_on_itself() {
    let diagnostics = diagnostics_of(PINNED_UNDER_EMBEDDED, "dev");
    let diagnostic = find(&diagnostics, "asks for a second application");
    assert_anchored(
        diagnostic,
        PINNED_UNDER_EMBEDDED,
        r#"application("second", anchor = "anchor-gear")"#,
    );
}

const CLUSTER_SCOPE: &str = r#"product(
    id = "spans-probe",
    name = "Spans Probe",
    version = "0.1.0",
    sources = [source(id = "somewhere", at = path("."))],
    profiles = [embedded(id = "dev")],
    default_profile = "dev",
    gears = [
        use_gear("app", source = "somewhere"),
        use_gear("cluster", source = "somewhere"),
    ],
    cluster_profiles = [
        cluster_profile(name = "main", cache = provider("not-a-provider")),
    ],
)
"#;

/// A cluster scope naming a provider nothing registers points at the scope.
///
/// The scope and not the `provider(...)` inside it, deliberately: the failing
/// primitive may be `cache`, `leader_election` or `lock`, and the resolver knows
/// which only as a name. Pointing at the cache binding when the lock is at fault
/// would underline the wrong line, and the `cluster_profile(...)` span contains
/// all three.
#[test]
fn a_cluster_scope_is_anchored_on_its_declaration() {
    let catalogue = support::cluster_catalogue(vec![(
        gearbox_ir::ClusterPrimitive::Cache,
        "main",
        &["cluster.cache.linearizable"],
    )]);
    let resolution = resolve_at(
        &catalogue,
        &intent(CLUSTER_SCOPE),
        &ProfileId::new("dev").unwrap(),
        Some(Path::new(PRODUCT_PATH)),
    );
    let diagnostics: Vec<Diagnostic> = resolution.diagnostics.iter().cloned().collect();
    let diagnostic = find(&diagnostics, "not-a-provider");
    assert_anchored(
        diagnostic,
        CLUSTER_SCOPE,
        r#"cluster_profile(name = "main""#,
    );
}

const UNKNOWN_ROLE: &str = r#"product(
    id = "spans-probe",
    name = "Spans Probe",
    version = "0.1.0",
    sources = [source(id = "somewhere", at = path("."))],
    profiles = [kubernetes(id = "k8s", discovery = "static")],
    default_profile = "k8s",
    gears = [
        use_gear("anchor-gear", source = "somewhere"),
    ],
    applications = [
        application("app", anchor = "anchor-gear", role = "bogus-role"),
    ],
)
"#;

/// An `application(...)` naming a role its anchor does not declare points at
/// the `application(...)` that named it.
#[test]
fn an_application_naming_an_unknown_role_is_anchored_on_itself() {
    let catalogue =
        support::catalogue_of(vec![support::gear_with_roles("anchor-gear", &["worker"])]);
    let resolution = resolve_at(
        &catalogue,
        &intent(UNKNOWN_ROLE),
        &ProfileId::new("k8s").unwrap(),
        Some(Path::new(PRODUCT_PATH)),
    );
    let diagnostics: Vec<Diagnostic> = resolution.diagnostics.iter().cloned().collect();
    let diagnostic = find(&diagnostics, "which declares no such role");
    assert_anchored(
        diagnostic,
        UNKNOWN_ROLE,
        r#"application("app", anchor = "anchor-gear", role = "bogus-role")"#,
    );
}

#[path = "support/resolve_fixtures.rs"]
mod support;

// --------------------------------------------------------------------------
// The gear side. These come out of a catalogue load, so they need a source
// root on disk -- but not a real crate: the fixture's `cargo(path = ...)` is
// what fails, which is the diagnostic being anchored.

const BROKEN_GEAR: &str = r#"gear(
    maturity = "stable",
    name = "probe-gear",
    category = "example",
    package = cargo(crate_name = "probe", lib = "probe", path = "/absolute/nowhere"),
)
"#;

/// An unresolvable `cargo(path = ...)` points at the `cargo(...)` call.
///
/// Anchored through `merge::bad_crate_path`, which five call sites share; before
/// the span was threaded it pointed at line 1 of whichever description
/// referenced the crate.
#[test]
fn an_unresolvable_crate_path_is_anchored_on_its_cargo_call() {
    let dir = std::env::temp_dir().join(format!("gbx-spans-{}", std::process::id()));
    drop(std::fs::remove_dir_all(&dir));
    std::fs::create_dir_all(&dir).expect("scratch source root");
    std::fs::write(dir.join("gear.gdl"), BROKEN_GEAR).expect("write the description");

    let root =
        gearbox_engine::SourceRoot::open(gearbox_ir::SourceId::new("probe").unwrap(), dir.clone())
            .expect("open the root");
    let scan = gearbox_engine::load_catalogue(&[root]);
    drop(std::fs::remove_dir_all(&dir));

    let diagnostics: Vec<Diagnostic> = scan.catalogue.diagnostics.iter().cloned().collect();
    let diagnostic = find(&diagnostics, "cannot be resolved");
    let location = diagnostic
        .location
        .as_ref()
        .expect("the diagnostic carries a location");
    assert!(location.uri.ends_with("gear.gdl"), "{}", location.uri);
    assert_ne!(
        location.range,
        Range::whole_file(),
        "the `cargo(...)` call records a span; the diagnostic must use it: {diagnostic:#?}"
    );
    assert_eq!(
        location.range.start.line,
        line_of(BROKEN_GEAR, "package = cargo("),
        "must be anchored on the `cargo(...)` line"
    );
}

const DOCS_GEAR_RS: &str = r#"
#[toolkit::gear(name = "probe-gear", capabilities = [system])]
pub struct ProbeGear;
"#;

const DOCS_MANIFEST: &str = "[package]\nname = \"probe\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n\
     [lib]\nname = \"probe\"\npath = \"src/lib.rs\"\n";

const DOCS_GEAR: &str = r#"gear(
    maturity = "stable",
    name = "probe-gear",
    category = "example",
    package = cargo(crate_name = "probe", lib = "probe", path = "."),
    docs = docs(prd = "missing.md"),
)
"#;

/// A `docs(prd = ...)` naming a file that does not exist points at the
/// `docs(...)` call.
///
/// Anchored through `docs::resolve_one`'s shared `at`, computed once from the
/// `docs(...)` record; before the span was threaded it pointed at line 1 of
/// whichever description declared it.
#[test]
fn a_missing_doc_path_is_anchored_on_its_docs_call() {
    let dir = std::env::temp_dir().join(format!("gbx-spans-docs-{}", std::process::id()));
    drop(std::fs::remove_dir_all(&dir));
    std::fs::create_dir_all(dir.join("src")).expect("scratch source root");
    std::fs::write(dir.join("gear.gdl"), DOCS_GEAR).expect("write the description");
    std::fs::write(dir.join("Cargo.toml"), DOCS_MANIFEST).expect("write the manifest");
    std::fs::write(dir.join("src/lib.rs"), DOCS_GEAR_RS).expect("write the crate");

    let root =
        gearbox_engine::SourceRoot::open(gearbox_ir::SourceId::new("probe").unwrap(), dir.clone())
            .expect("open the root");
    let scan = gearbox_engine::load_catalogue(&[root]);
    drop(std::fs::remove_dir_all(&dir));

    let diagnostics: Vec<Diagnostic> = scan.catalogue.diagnostics.iter().cloned().collect();
    let diagnostic = find(&diagnostics, "points at no file");
    let location = diagnostic
        .location
        .as_ref()
        .expect("the diagnostic carries a location");
    assert!(location.uri.ends_with("gear.gdl"), "{}", location.uri);
    assert_ne!(
        location.range,
        Range::whole_file(),
        "the `docs(...)` call records a span; the diagnostic must use it: {diagnostic:#?}"
    );
    assert_eq!(
        location.range.start.line,
        line_of(DOCS_GEAR, "docs = docs("),
        "must be anchored on the `docs(...)` line"
    );
}

const PLUGIN_SPANS: &str = r#"product(
    id = "spans-probe",
    name = "Spans Probe",
    version = "0.1.0",
    sources = [source(id = "somewhere", at = path("."))],
    profiles = [embedded(id = "dev")],
    default_profile = "dev",
    gears = [
        use_gear(
            "host-gear",
            source = "somewhere",
            plugins = [
                plugin("filler", config = {"vendor": 5}),
            ],
        ),
    ],
)
"#;

/// A plugin's bad `vendor` points at the `plugin(...)`, not the host's `use_gear`.
///
/// The anchor moved when `PluginSelection` gained a span. The host's
/// `use_gear(...)` contains the entry, so the old anchor was true but coarse --
/// on a `use_gear` spanning six lines it underlined all of them.
#[test]
fn a_plugin_config_type_error_is_anchored_on_its_plugin_entry() {
    // Through `plugin_select::check` directly: this check runs in `validate`,
    // not in `resolve`, so `diagnostics_of` would never reach it.
    let mut diagnostics = gearbox_ir::Diagnostics::new();
    let uri = gearbox_ir::file_uri(Path::new(PRODUCT_PATH));
    drop(gearbox_engine::check_plugins(
        &Catalogue::default(),
        &intent(PLUGIN_SPANS),
        &uri,
        &mut diagnostics,
    ));
    let all: Vec<Diagnostic> = diagnostics.iter().cloned().collect();
    let diagnostic = find(&all, "`vendor` must be a string");
    assert_anchored(diagnostic, PLUGIN_SPANS, r#"plugin("filler", config ="#);
}

const PLUGIN_TWICE: &str = r#"product(
    id = "spans-probe",
    name = "Spans Probe",
    version = "0.1.0",
    sources = [source(id = "somewhere", at = path("."))],
    profiles = [embedded(id = "dev")],
    default_profile = "dev",
    gears = [
        use_gear(
            "host-gear",
            source = "somewhere",
            plugins = [
                plugin("filler"),
                plugin("filler", config = {"priority": 2}),
            ],
        ),
    ],
)
"#;

/// The same plugin listed twice points at the second entry.
///
/// This diagnostic had no test at all before -- its message appeared in no test
/// in the repository -- so neither the collision nor its anchor was pinned.
#[test]
fn a_plugin_selected_twice_is_anchored_on_the_duplicate() {
    let diagnostics = eval_diagnostics(PLUGIN_TWICE);
    let diagnostic = find(&diagnostics, "is selected twice for");
    assert_anchored(
        diagnostic,
        PLUGIN_TWICE,
        r#"plugin("filler", config = {"priority": 2})"#,
    );
}
