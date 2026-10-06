//! Where a host function puts its result.
//!
//! `starlark`'s `Module::with_temp_heap` hands out a `Module` under a
//! higher-ranked lifetime, so no `Value<'v>` can escape the closure. Results
//! therefore leave through a [`GdlSink`] the caller owns *outside* the closure
//! and installs as `Evaluator::extra`; the host functions write owned Rust data
//! into it, and the caller reads it after evaluation returns. This is the
//! pattern starlark's own documentation uses.
//!
//! The sink is also where the "exactly one declaration per file" rule lives:
//! `gear()` and `product()` write once, and a second call is an error rather
//! than a silent overwrite.

// See the note on `unsafe_code` in the root Cargo.toml.
#![allow(
    unsafe_code,
    reason = "starlark's ProvidesStaticType is an unsafe trait and its derive is \
              required to install this type as Evaluator::extra"
)]

use std::cell::RefCell;

use gearbox_ir::Diagnostics;
use starlark::any::ProvidesStaticType;

use crate::records::{ConsumeRecord, ProvideRecord, RoleRecord};

/// The raw, still-untyped result of one `gear(...)` call.
///
/// Host functions cannot build a [`GearDescriptor`] directly: converting needs
/// the source identity and the description's own path, which only the caller
/// knows. So `gear()` records what the file said, and
/// [`crate::engine`] converts.
#[derive(Debug, Clone, Default)]
pub struct GearDecl {
    /// How far the gear has got. `gear()` refuses a description without one,
    /// so `None` appears only on a `GearDecl` built by hand.
    pub maturity: Option<Maturity>,
    /// The id, written only by a design gear.
    ///
    /// A gear with code has its id projected from `#[toolkit::gear(name =
    /// ...)]`, and restating it is GBX0210. A design gear has no attribute to
    /// project it from, so it is the one place the id is declared.
    pub id: Option<String>,
    pub name: Option<String>,
    pub description: Option<String>,
    pub category: Option<String>,
    pub visibility: Option<String>,
    pub package: Option<crate::records::CargoRecord>,
    /// Where this gear's SDK crate is, when it has one.
    ///
    /// A locator, not a restatement: the SDK declares the GTS types this gear
    /// exposes and the traits its extension points name, and nothing in the
    /// gear's own crate says where that SDK is. A plugin does not write one:
    /// the host's SDK is the host's.
    pub sdk: Option<crate::records::CargoRecord>,
    /// The points this gear lets plugins fill, keyed by GTS spec segment.
    pub extension_points: Vec<crate::records::ExtensionPointRecord>,
    /// The spec segment of the point this gear implements, when it is a plugin.
    pub implements: Option<String>,
    /// Overrides the convention-based search for this gear's documents.
    pub docs: Option<crate::records::DocsRecord>,
    pub provides: Vec<ProvideRecord>,
    pub consumes: Vec<ConsumeRecord>,
    pub requires: Vec<crate::records::ClusterRequireRecord>,
    pub serves: Vec<crate::records::EndpointRecord>,
    /// Where the cluster backend plugin crates live, so their `PROVIDER_NAME`
    /// consts and capability declarations can be projected.
    ///
    /// Declared rather than projected for the same reason `sdk` is: telling the
    /// scanner where to look is not restating the fact it will find.
    pub cluster_plugins: Vec<crate::records::ClusterPluginRecord>,
    pub declared_roles: Vec<RoleRecord>,
    pub config_schema: Option<crate::records::ConfigRecord>,
    /// The Cargo features this gear offers an integrator, curated and scoped.
    ///
    /// **`None` and `Some([])` are different answers**, which is why this is an
    /// option and not a list. Absent means nobody has curated this gear, and a
    /// client falls back to the projected `available_features` while saying it
    /// is uncurated. An empty list is a curation: `types-registry` declares one
    /// feature, `integration`, which wants a Docker daemon -- "there is nothing
    /// here for you" is the true answer and it has to be expressible.
    pub cargo_features: Option<Vec<crate::records::FeatureRecord>>,
    /// Where the `gear(...)` call was written in the description.
    pub declared_at: Option<gearbox_ir::Location>,
}

/// `maturity = "..."` on a `gear(...)`. Required, with no default.
///
/// **No default, because the safe one does not exist.** `stable` as a default
/// turns a description that forgot the field into a promise nobody made;
/// anything lower turns it into an accusation. So the author says which.
///
/// `Design` is a gear described before it has code: it names an id, says what
/// the gear is for and where its documents are, and stops there. It has no
/// `package`, so nothing is projected and nothing can be resolved into a
/// product; the catalogue lists it apart from the gears that can be used.
/// The other four describe code, and are what [`gearbox_ir::Maturity`] keeps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Maturity {
    Design,
    Code(gearbox_ir::Maturity),
}

impl Maturity {
    /// The spellings `maturity = ...` accepts, in order of increasing promise.
    pub const SPELLINGS: &[&str] = &["design", "experimental", "preview", "stable", "deprecated"];

    #[must_use]
    pub fn parse(spelling: &str) -> Option<Self> {
        Some(match spelling {
            "design" => Self::Design,
            "experimental" => Self::Code(gearbox_ir::Maturity::Experimental),
            "preview" => Self::Code(gearbox_ir::Maturity::Preview),
            "stable" => Self::Code(gearbox_ir::Maturity::Stable),
            "deprecated" => Self::Code(gearbox_ir::Maturity::Deprecated),
            _ => return None,
        })
    }
}

/// The raw result of one `product(...)` call.
///
/// Mirrors [`GearDecl`]: host functions cannot build a `ProductIntent` because
/// it carries the description's own path, which only the caller knows. So
/// `product()` records what the file said and [`crate::engine`] converts,
/// validating ids and profile references on the way.
#[derive(Debug, Clone, Default)]
pub struct ProductDecl {
    pub id: String,
    pub display_name: String,
    pub version: String,
    pub default_profile: String,
    pub sources: Vec<crate::records::SourceRecord>,
    /// Where this product's template overlay lives, when it says.
    ///
    /// Absent means the convention: a `templates/` directory beside the
    /// description. Declaring it exists so the directory can live *outside* the
    /// product -- one house template set shared by twenty products rather than
    /// twenty copies of it, which is what the convention alone forced.
    pub templates: Option<crate::records::SourceAtRecord>,
    /// The directory generated application crates go under, when it says.
    ///
    /// Absent means `apps/`. A checkout generated before the layout was
    /// configurable can say `processes` and keep the tree it has.
    pub layout: Option<String>,
    pub profiles: Vec<crate::records::ProfileRecord>,
    pub gears: Vec<crate::records::UseGearRecord>,
    pub bindings: Vec<crate::records::BindRecord>,
    pub cluster_profiles: Vec<crate::records::ClusterProfileRecord>,
    pub applications: Vec<crate::records::ApplicationRecord>,
    pub preferences: Vec<crate::records::PreferenceRecord>,
}

/// Collects one file's declaration and any diagnostics raised while evaluating it.
#[derive(Debug, Default, ProvidesStaticType)]
pub struct GdlSink {
    gear: RefCell<Option<GearDecl>>,
    product: RefCell<Option<ProductDecl>>,
    diagnostics: RefCell<Diagnostics>,
    /// Set when a second `gear()`/`product()` call is seen, so the engine can
    /// report GBX0105 without the host function needing to fail the evaluation.
    duplicate: RefCell<bool>,
}

impl GdlSink {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Record the file's `gear(...)` declaration.
    ///
    /// A second call marks the file as duplicated rather than overwriting: the
    /// first declaration is the one a reader would expect to win, and the
    /// engine turns the flag into GBX0105.
    pub fn set_gear(&self, decl: GearDecl) {
        let mut slot = self.gear.borrow_mut();
        if slot.is_some() {
            *self.duplicate.borrow_mut() = true;
        } else {
            *slot = Some(decl);
        }
    }

    /// Record the file's `product(...)` declaration. Same write-once rule.
    pub fn set_product(&self, intent: ProductDecl) {
        let mut slot = self.product.borrow_mut();
        if slot.is_some() {
            *self.duplicate.borrow_mut() = true;
        } else {
            *slot = Some(intent);
        }
    }

    #[must_use]
    pub fn saw_duplicate(&self) -> bool {
        *self.duplicate.borrow()
    }

    /// Take the recorded gear declaration, leaving the sink empty.
    #[must_use]
    pub fn take_gear(&self) -> Option<GearDecl> {
        self.gear.borrow_mut().take()
    }

    /// Take the recorded product intent, leaving the sink empty.
    #[must_use]
    pub fn take_product(&self) -> Option<ProductDecl> {
        self.product.borrow_mut().take()
    }

    /// Take the collected diagnostics, leaving the sink empty.
    #[must_use]
    pub fn take_diagnostics(&self) -> Diagnostics {
        std::mem::take(&mut self.diagnostics.borrow_mut())
    }
}
