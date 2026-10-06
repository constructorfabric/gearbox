//! What the platform's attribute says and this tool does not read.
//!
//! One claim, and it exists because a field promised it for months without
//! anyone performing it. `ProjectedGear::unmodelled` records an argument the
//! `#[toolkit::gear]` macro accepts and this parser does not model, and its doc
//! says that is "so a future macro argument surfaces as a known gap instead of a
//! silent omission". It had one writer, no reader outside its own unit test, and
//! no diagnostic code.
//!
//! The state is reachable only in a skew window -- the macro refuses an argument
//! it does not know, so the platform lands one first and this parser catches up
//! after -- which is exactly how `one_per_installation` arrived. The fixture
//! here writes what that window looks like from this side: Rust carrying an
//! argument the catalogue cannot account for.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "clippy.toml's allow-unwrap-in-tests covers #[test] fns but not the helpers here"
)]

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use gearbox_engine::{SourceRoot, load_catalogue};
use gearbox_ir::{Catalogue, DiagnosticCode, GearId, Severity, SourceId};

static NEXT: AtomicUsize = AtomicUsize::new(0);

const MANIFEST: &str = "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n\
     [lib]\nname = \"demo\"\npath = \"src/lib.rs\"\n";

const GEAR_GDL: &str = r#"
gear(
    maturity = "stable",
    name = "Demo",
    description = "d",
    category = "core-functionality",
    visibility = "internal",
    package = cargo(crate_name = "demo", lib = "demo", path = "."),
)
"#;

/// One root holding one gear whose attribute carries `args`.
fn catalogue(args: &str) -> Catalogue {
    let nth = NEXT.fetch_add(1, Ordering::Relaxed);
    let root: PathBuf =
        std::env::temp_dir().join(format!("gbx-projection-gaps-{}-{nth}", std::process::id()));
    drop(std::fs::remove_dir_all(&root));
    let crate_dir = root.join("demo");
    std::fs::create_dir_all(crate_dir.join("src")).unwrap();
    std::fs::write(crate_dir.join("gear.gdl"), GEAR_GDL).unwrap();
    std::fs::write(crate_dir.join("Cargo.toml"), MANIFEST).unwrap();
    std::fs::write(
        crate_dir.join("src/lib.rs"),
        format!("#[toolkit::gear({args})]\npub struct DemoGear;\n"),
    )
    .unwrap();

    let source = SourceRoot::open(SourceId::new("demo").unwrap(), &root).unwrap();
    load_catalogue(&[source]).catalogue
}

fn found(catalogue: &Catalogue) -> Option<&gearbox_ir::Diagnostic> {
    catalogue
        .diagnostics
        .iter()
        .find(|d| d.code == DiagnosticCode::GapUnmodelledGearArgument)
}

#[test]
fn an_argument_this_tool_does_not_model_is_reported() {
    let catalogue = catalogue(r#"name = "demo", capabilities = [system], something_new = "v""#);
    let d = found(&catalogue).expect("GBX0608");
    assert_eq!(d.severity, Severity::Warning);
    assert!(d.message.contains("something_new"), "{}", d.message);
    // The range's own contract: a claim about another repository has to cite
    // what it read. `every_runtime_gap_code_requires_evidence` enforces the
    // column; this enforces a value in it.
    assert!(d.evidence.is_some(), "{d:?}");
    assert!(d.help.is_some(), "{d:?}");

    // And the gear still resolves: this tool is behind, the description is not
    // wrong, so nothing is refused.
    assert!(
        catalogue.gears.contains_key(&GearId::new("demo").unwrap()),
        "a gap is a warning, not a rejection"
    );
    assert!(
        !catalogue.diagnostics.has_errors(),
        "{:#?}",
        catalogue.diagnostics
    );
}

#[test]
fn several_unmodelled_arguments_are_one_report_naming_all_of_them() {
    let catalogue = catalogue(r#"name = "demo", first_new = 1, second_new = "two""#);
    let d = found(&catalogue).expect("GBX0608");
    assert!(
        d.message.contains("first_new") && d.message.contains("second_new"),
        "{}",
        d.message
    );
    assert_eq!(
        catalogue
            .diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::GapUnmodelledGearArgument)
            .count(),
        1,
        "one gear is one gap, however many arguments it is"
    );
}

#[test]
fn an_attribute_this_tool_models_entirely_is_silent() {
    // Every argument the macro takes and this parser reads, so the fallthrough
    // is empty and there is nothing to say. Without this the test above would
    // pass on a check that fired for any gear at all.
    let catalogue = catalogue(
        r#"name = "demo", capabilities = [system], deps = [], one_per_installation = false"#,
    );
    assert!(found(&catalogue).is_none(), "{:#?}", catalogue.diagnostics);
}
