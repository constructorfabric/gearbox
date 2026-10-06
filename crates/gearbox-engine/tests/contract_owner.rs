//! A contract's owner is a name, and the catalogue has to answer to it.
//!
//! `#[toolkit::contract(gear = "...")]` is a free string. It becomes the first
//! segment of the contract's own id and the key `contract_family` and
//! `other_major` group on, and until now nothing asked whether it named
//! anything. A typo, or a gear nobody described, produced a contract whose
//! identity no other contract could match -- silently.
//!
//! The third test is the one worth keeping honest: a role's `directory_name`
//! satisfies the check. ADR `cpt-gearbox-adr-role-qualified-names` decides that
//! a contract may legitimately answer to a role rather than to a gear, so a
//! check written as "the owner is a gear id" would have to be rewritten the
//! moment roles do anything. This one is written as "a name the catalogue
//! answers to" and does not.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "clippy.toml's allow-unwrap-in-tests covers #[test] fns but not the helpers here"
)]

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use gearbox_engine::{SourceRoot, load_catalogue};
use gearbox_ir::{Catalogue, DiagnosticCode, SourceId};

static NEXT: AtomicUsize = AtomicUsize::new(0);

const SDK_MANIFEST: &str = "[package]\nname = \"thing-sdk\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n\
     [lib]\nname = \"thing_sdk\"\npath = \"src/lib.rs\"\n";

const PROVIDER_MANIFEST: &str = "[package]\nname = \"provider\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n\
     [lib]\nname = \"provider\"\npath = \"src/lib.rs\"\n";

const PROVIDER_RS: &str = r#"
#[toolkit::gear(name = "provider", capabilities = [system])]
pub struct ProviderGear;
"#;

/// A tree of two crates: an SDK declaring one contract, and a gear providing it.
///
/// `owner` is what the contract attribute names, and `roles` is whatever the
/// provider's description should declare -- the two knobs each test turns.
fn root(owner: &str, roles: &str) -> PathBuf {
    let nth = NEXT.fetch_add(1, Ordering::Relaxed);
    let root =
        std::env::temp_dir().join(format!("gbx-contract-owner-{}-{nth}", std::process::id()));
    drop(std::fs::remove_dir_all(&root));

    let sdk = root.join("thing-sdk");
    std::fs::create_dir_all(sdk.join("src")).unwrap();
    std::fs::write(sdk.join("Cargo.toml"), SDK_MANIFEST).unwrap();
    std::fs::write(
        sdk.join("src/lib.rs"),
        format!(
            r#"
#[toolkit::contract(gear = "{owner}", version = "v1")]
pub trait ThingApi: Send + Sync {{}}
"#
        ),
    )
    .unwrap();

    let provider = root.join("provider");
    std::fs::create_dir_all(provider.join("src")).unwrap();
    std::fs::write(provider.join("Cargo.toml"), PROVIDER_MANIFEST).unwrap();
    std::fs::write(provider.join("src/lib.rs"), PROVIDER_RS).unwrap();
    std::fs::write(
        provider.join("gear.gdl"),
        format!(
            r#"
gear(
    maturity = "stable",
    name = "Provider",
    description = "d",
    category = "core-functionality",
    visibility = "internal",
    package = cargo(crate_name = "provider", lib = "provider", path = "."),
    provides = [
        provide(
            contract = "ThingApi",
            rust = "thing_sdk::ThingApi",
            sdk = cargo(crate_name = "thing-sdk", lib = "thing_sdk", path = "../thing-sdk"),
            local = "Self::build_local",
            rest = rest(base_path = "/thing/v1"),
        ),
    ],
    {roles}
)
"#
        ),
    )
    .unwrap();
    root
}

/// The same tree plus a consumer whose directory name sorts *before* the
/// provider's, so the walk reaches the partial copy first.
fn root_with_consumer(owner: &str, roles: &str) -> PathBuf {
    let root = root(owner, roles);
    let consumer = root.join("aaa-consumer");
    std::fs::create_dir_all(consumer.join("src")).unwrap();
    std::fs::write(
        consumer.join("Cargo.toml"),
        "[package]\nname = \"aaa-consumer\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n\
         [lib]\nname = \"aaa_consumer\"\npath = \"src/lib.rs\"\n",
    )
    .unwrap();
    std::fs::write(
        consumer.join("src/lib.rs"),
        "\n#[toolkit::gear(name = \"aaa-consumer\", capabilities = [system])]\npub struct AaaConsumerGear;\n",
    )
    .unwrap();
    std::fs::write(
        consumer.join("gear.gdl"),
        r#"
gear(
    maturity = "stable",
    name = "Consumer",
    description = "d",
    category = "core-functionality",
    visibility = "internal",
    package = cargo(crate_name = "aaa-consumer", lib = "aaa_consumer", path = "."),
    consumes = [
        consume(
            contract = "ThingApi",
            rust = "thing_sdk::ThingApi",
            sdk = cargo(crate_name = "thing-sdk", lib = "thing_sdk", path = "../thing-sdk"),
            from_ = "provider",
        ),
    ],
)
"#,
    )
    .unwrap();
    root
}

fn catalogue(owner: &str, roles: &str) -> Catalogue {
    let path = root(owner, roles);
    let source = SourceRoot::open(SourceId::new("demo").unwrap(), &path).unwrap();
    load_catalogue(&[source]).catalogue
}

fn codes(catalogue: &Catalogue) -> Vec<DiagnosticCode> {
    catalogue.diagnostics.iter().map(|d| d.code).collect()
}

#[test]
fn an_owner_no_description_declares_is_reported() {
    let catalogue = catalogue("provider-ingest", "");
    let found: Vec<_> = catalogue
        .diagnostics
        .iter()
        .filter(|d| d.code == DiagnosticCode::TopologyUnknownContractOwner)
        .collect();

    // One per contract, not one per gear that mentions it: the mistake is in the
    // attribute, and pointing at each `gear.gdl` would multiply one fix.
    assert_eq!(found.len(), 1, "{:?}", codes(&catalogue));
    assert!(
        found[0].message.contains("provider-ingest"),
        "the message names the owner: {}",
        found[0].message
    );
    assert!(
        found[0]
            .help
            .as_deref()
            .unwrap_or_default()
            .contains("thing-sdk"),
        "the help names the crate the attribute is in: {:?}",
        found[0].help
    );
}

#[test]
fn a_declared_role_directory_name_satisfies_the_check() {
    // The forward-compatibility claim, as a test rather than a promise.
    let catalogue = catalogue(
        "provider-ingest",
        r#"roles = [role(name = "ingest", directory_name = "provider-ingest")],"#,
    );
    let found = codes(&catalogue);
    assert!(
        !found.contains(&DiagnosticCode::TopologyUnknownContractOwner),
        "a role's directory name is a name the catalogue answers to: {found:?}"
    );
    // And the role costs nothing at load: whether a product can deploy it is
    // GBX0318, which only a resolution knows.
    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn an_owner_that_is_the_declaring_gear_is_silent() {
    let found = codes(&catalogue("provider", ""));
    assert!(
        !found.contains(&DiagnosticCode::TopologyUnknownContractOwner),
        "the ordinary tree must stay quiet: {found:?}"
    );
}

#[test]
fn a_consumers_stub_does_not_beat_the_providers_copy_when_the_owner_is_a_role() {
    // Defect B, and it is silent rather than loud: no diagnostic marks it.
    //
    // `aaa-consumer` sorts before `provider`, so the walk reaches its partial
    // copy first. The old rule asked `contract.owner == declared_by`, which is
    // false for *both* gears once the owner is a role name, so it fell back to
    // "first writer wins" and the consumer's stub -- `rest: None` -- became the
    // catalogue's copy of a contract that has a REST projection.
    let path = root_with_consumer(
        "provider-ingest",
        r#"roles = [role(name = "ingest", directory_name = "provider-ingest")],"#,
    );
    let source = SourceRoot::open(SourceId::new("demo").unwrap(), &path).unwrap();
    let catalogue = load_catalogue(&[source]).catalogue;

    let contract = catalogue
        .contracts
        .values()
        .find(|c| c.base_name == "ThingApi")
        .expect("the contract is in the catalogue");
    assert!(
        contract.rest.is_some(),
        "the provider's copy carries the REST projection and must win; \
         `rest: None` means the consumer's stub did"
    );
}
