//! Plugin extension points and implements: declared in the description, checked here.
//!
//! **The role is declared, not read.** A host writes
//! `extension_points = [extension_point("<spec>", trait = "...")]`; a plugin
//! writes `implements = "<spec>"`. The key is the GTS spec every plugin family
//! registers instances under and the host selects by, so two points over one
//! trait stay two points (the ledger's rate provider and bss-rate-provider's
//! sources both implement `bss_ledger_sdk::RateProviderV1`).
//!
//! This used to be inferred -- a point was a `pub trait` with `Plugin` in its
//! name, a plugin a crate implementing one -- and the corpus broke that five
//! ways: proxies and built-ins implementing their host's own trait, a trait with
//! no `Plugin` in it, one crate with three gears, two points over one trait,
//! mocks in test support. So the code now only *checks* a declaration:
//!
//! - a point's spec must be a `PluginV1`-derived GTS type the gear's SDK
//!   declares, and its trait a `pub trait` in the SDK the point names
//!   (GBX0516 otherwise);
//! - a plugin's fill is joined to its host once every gear is known, in
//!   `catalogue.rs` (GBX0519 when no described gear declares the spec);
//! - the traits a plugin's crate implements are carried as evidence for a
//!   warning, never as the answer (GBX0526).
//!
//! What stays projected is what was never wrong: the `vendor`/`priority`
//! defaults each side compiles in.

use std::collections::BTreeSet;

use gearbox_gdl::GearDecl;
use gearbox_gdl::engine::FileIdentity;
use gearbox_ir::{
    Diagnostic, DiagnosticCode, Diagnostics, ExtensionPointDecl, GtsTypeDecl, Location, PluginImpl,
};
use gearbox_project::RustFile;

/// The GTS base every plugin spec derives from.
///
/// A description writes only a spec's own segment; the catalogue stores the
/// full chain, which is also what makes the declaration a check that the type
/// really is a plugin spec rather than any GTS type the SDK happens to declare.
pub const PLUGIN_BASE: &str = "cf.toolkit.plugins.plugin.v1~";

/// The plugin half of one gear's projection.
#[derive(Debug, Default)]
pub struct PluginProjection {
    /// Points this gear lets plugins fill, each checked against its SDK.
    pub extension_points: Vec<ExtensionPointDecl>,
    /// The point this gear implements, if it is a plugin. `point` is `None` here and
    /// set by the catalogue once the host is known.
    pub implements: Option<PluginImpl>,
    /// The vendor string this gear's config selects by. Hosts only.
    pub vendor_selector: Option<String>,
    /// The traits this gear's own files implement outside tests; evidence for
    /// the GBX0526 check, never the source of the role.
    pub implemented: BTreeSet<String>,
}

/// Where a point's trait is looked for: the crate, already scanned.
pub struct TraitSdk<'a> {
    pub record: &'a gearbox_gdl::records::CargoRecord,
    pub files: &'a [RustFile],
}

/// Project the plugin facts for one gear.
///
/// `files` is the gear's own crate, narrowed to what this gear owns when the
/// crate declares several; `gts_types` are those its own SDK declares.
/// `trait_sdks` is aligned with `decl.extension_points`: where each point's
/// trait lives -- the point's own `sdk` when it names one, the gear's otherwise
/// -- or `None` when that crate could not be read, which the caller has
/// already reported.
pub fn project(
    identity: &FileIdentity,
    decl: &GearDecl,
    files: &[RustFile],
    gts_types: &[GtsTypeDecl],
    trait_sdks: &[Option<TraitSdk<'_>>],
    diagnostics: &mut Diagnostics,
) -> PluginProjection {
    let uri = identity.uri.as_str();
    let own_default = gearbox_project::project_vendor_default(files);

    let mut extension_points = Vec::new();
    for (index, point) in decl.extension_points.iter().enumerate() {
        let at = Location::or_file(point.declared_at.as_ref(), uri);
        let spec = format!("{PLUGIN_BASE}{}", point.spec);

        if decl.sdk.is_none() {
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::PluginPointUndetermined,
                    format!(
                        "`extension_point(\"{}\")` needs `sdk = cargo(...)` on this gear: the \
                         spec is checked against the GTS types that SDK declares",
                        point.spec
                    ),
                    "add the gear's own `sdk = cargo(...)` locator",
                )
                .at(at),
            );
            continue;
        }
        if !gts_types.iter().any(|t| t.type_id == spec) {
            let known: Vec<&str> = gts_types
                .iter()
                .filter_map(|t| t.type_id.strip_prefix(PLUGIN_BASE))
                .collect();
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::PluginPointUndetermined,
                    format!(
                        "`extension_point(\"{}\")` names no plugin spec this gear's sdk declares; \
                         {}",
                        point.spec,
                        if known.is_empty() {
                            "it declares none derived from `PluginV1`".to_owned()
                        } else {
                            format!("it declares: {}", known.join(", "))
                        }
                    ),
                    "write the segment of a `#[gts_type_schema(base = PluginV1, ...)]` type in \
                     the gear's sdk",
                )
                .at(at),
            );
            continue;
        }

        let Some(sdk) = trait_sdks.get(index).and_then(Option::as_ref) else {
            continue;
        };
        let traits = gearbox_project::public_traits(sdk.files);
        let ident = point
            .trait_ident
            .rsplit("::")
            .next()
            .unwrap_or(&point.trait_ident);
        if !traits.contains(ident) {
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::PluginPointUndetermined,
                    format!(
                        "`extension_point(\"{}\", trait = \"{}\")` names no `pub trait` in `{}`",
                        point.spec, point.trait_ident, sdk.record.crate_name
                    ),
                    if point.sdk.is_some() {
                        "check the trait's spelling, or the point's `sdk = cargo(...)`".to_owned()
                    } else {
                        "check the trait's spelling; a trait in another crate needs `sdk = \
                         cargo(...)` on the extension_point"
                            .to_owned()
                    },
                )
                .at(at),
            );
            continue;
        }

        // `path` as written is relative to the description; the catalogue
        // records it relative to the source root, like every other locator.
        let sdk_ref = crate::merge::cargo_ref(
            sdk.record,
            &identity.gdl_path.parent(),
            "extension_point.sdk",
            uri,
            diagnostics,
        );
        extension_points.push(ExtensionPointDecl {
            spec,
            trait_ident: ident.to_owned(),
            sdk_lib: sdk.record.lib_ident.clone(),
            sdk: sdk_ref,
            selector: point.selector.clone(),
        });
    }

    let fill = decl.implements.as_ref().map(|segment| PluginImpl {
        spec: format!("{PLUGIN_BASE}{segment}"),
        point: None,
        default_vendor: own_default.vendor.clone(),
        default_priority: own_default.priority,
    });

    // **Declared, when the description says where it is.** A config may hold
    // more than one `vendor`: account-management selects its IdP plugin by
    // `idp.vendor` and registers itself as a tenant-resolver plugin under
    // `tr_plugin.vendor`. The first `vendor` default the reader met was the
    // second one, and every product with the host failed GBX0512 against a
    // runtime that would have found its plugin. So `extension_point(selector =
    // ...)` names the field, and its default is read at that path.
    //
    // Without one, the old rule: a selector only on a gear that is purely a
    // host. A gear that is both -- bss-rate-provider -- has one config and two
    // vendors in it, and reporting the wrong one would make the vendor-match
    // check wrong, so it reports none.
    let declared: Vec<(&str, Location)> = decl
        .extension_points
        .iter()
        .filter_map(|p| {
            p.selector
                .as_deref()
                .map(|s| (s, Location::or_file(p.declared_at.as_ref(), uri)))
        })
        .collect();
    let vendor_selector = match declared.first() {
        Some((path, at)) => {
            if let Some((other, other_at)) = declared.iter().find(|(p, _)| p != path) {
                // One selector per gear: the catalogue keeps one value, and the
                // runtime's hosts read one field. Two would need a model this
                // one does not have.
                diagnostics.push(
                    Diagnostic::error(
                        DiagnosticCode::PluginPointUndetermined,
                        format!(
                            "this gear's extension points name two selectors, `{path}` and \
                             `{other}`; a host selects all its points by one vendor field"
                        ),
                        "name the same `selector` on every `extension_point`, or drop it from \
                         all but one",
                    )
                    .at(other_at.clone()),
                );
                None
            } else {
                match gearbox_project::project_field_str_default(files, path) {
                    Ok(value) => value,
                    Err(why) => {
                        diagnostics.push(
                            Diagnostic::error(
                                DiagnosticCode::PluginPointUndetermined,
                                format!("`selector = \"{path}\"` cannot be read: {why}"),
                                "name the config field the host selects its plugin by, as a \
                                 dotted path from the gear's config struct",
                            )
                            .at(at.clone()),
                        );
                        None
                    }
                }
            }
        }
        None => (!extension_points.is_empty() && fill.is_none())
            .then(|| own_default.vendor.clone())
            .flatten(),
    };

    PluginProjection {
        extension_points,
        implements: fill,
        vendor_selector,
        implemented: gearbox_project::implemented_traits(files),
    }
}
