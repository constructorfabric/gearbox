// The PRD requirements about what the catalogue projects, checked where a person
// reads them: the Gear detail panel.
//
// These are the facts the project has already got wrong three times --
// `has_extension_point`, a provider's transports, a plugin's GTS types -- each
// time by reading a plausible second source instead of the Rust that owns them.
// So the assertions are written against the specific gear where a wrong answer
// would look right.

import { expect, test } from "../fixtures/studio";

test.describe("projected facts", () => {
  test("a gear's id, capabilities and co-location come from its Rust attributes [PRD cpt-gearbox-fr-catalogue-projection]", async ({
    studio,
  }) => {
    const detail = await studio.detailOf("API Gateway");
    expect(detail).not.toBeNull();
    expect(detail).toContain("api-gateway");
    // Named "co-located with", not "depends on": these edges are link-time and
    // the resolver can never sever them.
    expect(detail).toContain("co-located with");
    expect(detail).toMatch(/authn-resolver/);
    expect(detail).toMatch(/grpc-hub/);
  });

  test("a provider's transports are what it wires up, not what the contract allows [PRD cpt-gearbox-fr-transport-projection]", async ({
    studio,
  }) => {
    const detail = await studio.detailOf("Payments (example provider)");
    expect(detail).not.toBeNull();
    expect(detail).toContain("PaymentApi");
    // api-contracts-sdk declares `PaymentApiGrpc`, so gRPC is possible for v1 --
    // but the gear's own `#[toolkit::provides]` omits it, because that client
    // sits behind an opt-in Cargo feature. "grpc" appearing here would mean the
    // catalogue had gone back to reading the contract's possibilities as the
    // provider's offer.
    expect(detail).not.toMatch(/grpc/);
  });

  test("a host gear shows the extension point its SDK declares [PRD cpt-gearbox-fr-plugin-extension-points]", async ({
    studio,
  }) => {
    const detail = await studio.detailOf("Authentication Resolver");
    expect(detail).not.toBeNull();
    expect(detail).toContain("extension points");
    // The trait ident as written. Deriving a short name would repeat the
    // `ClusterProfile` mistake: kebab-casing `AuthNResolverPluginClient` gives
    // `auth-n-resolver-...`, the same `AuthN` -> `auth-n` split GBX0206 catches.
    expect(detail).toContain("AuthNResolverPluginClient");
    expect(detail).toMatch(/selects vendor/);
  });

  test("a plugin shows which point it implements and its compiled-in vendor [PRD cpt-gearbox-fr-plugin-extension-points]", async ({
    studio,
  }) => {
    const detail = await studio.detailOf("OIDC AuthN Plugin");
    expect(detail).not.toBeNull();
    expect(detail).toMatch(/implements/);
    expect(detail).toContain("AuthNResolverPluginClient");
    // This gear is the evidence for the "both spellings" half of the
    // requirement: `oidc-authn-plugin` declares its default through
    // `#[serde(default = "default_vendor")]` and a free function, not through
    // `impl Default`. A reader that handled only `impl Default` would report "no
    // default" here -- and a missing default is what the vendor-mismatch check
    // keys on, so that would be a wrong answer rather than a gap.
    expect(detail).toMatch(/as vendor/);
    expect(detail).toMatch(/priority 100/);
  });

  test("the host's selector and the plugin's default vendor can be compared [PRD cpt-gearbox-fr-plugin-selection]", async ({
    studio,
  }) => {
    // Both halves are rendered, and nothing checked that they agree. The
    // requirement is that a mismatch is an error, and the reason it can happen
    // at all is that each side reads the string from its own config, so each has
    // its own compiled-in default.
    const host = await studio.factsOf("Authentication Resolver");
    const plugin = await studio.factsOf("OIDC AuthN Plugin");
    // Read structurally: the vendor is the last `<code>` run of the row, because
    // `textContent` runs the next row's first word straight onto it.
    const selector = host?.["extension points"]?.codes.at(-1);
    const provided = plugin?.implements?.codes.at(-1);
    expect(selector, "the host renders no vendor selector").toBeTruthy();
    expect(provided, "the plugin renders no default vendor").toBeTruthy();
    expect(provided).toBe(selector);
  });

  test("a gear's GTS types are attributed to the gear whose SDK declares them [PRD cpt-gearbox-fr-gts-types]", async ({
    studio,
  }) => {
    const host = await studio.detailOf("Authentication Resolver");
    expect(host).toContain("GTS types");
    expect(host).toContain("cf.core.authn_resolver.plugin.v1");

    // The other half of the requirement, and the one that has been wrong before:
    // the plugin *references* the host's type with `gts_id!`, which is not a
    // declaration, so the type must not be attributed to it.
    const plugin = await studio.detailOf("OIDC AuthN Plugin");
    expect(plugin).not.toContain("GTS types");
  });

  test("a gear with documents shows PRD, DESIGN and ADR links [PRD cpt-gearbox-fr-gear-documents]", async ({
    studio,
  }) => {
    await studio.detailOf("Cluster Coordination");
    const links = await studio.page.evaluate(() =>
      Array.from(document.querySelectorAll(".gbx-links a")).map((a) => (a.textContent ?? "").trim()),
    );
    expect(links).toContain("PRD");
    expect(links).toContain("DESIGN");
    expect(links.some((l) => l.startsWith("ADR"))).toBe(true);
  });

  test("categories come from the platform, not from us [PRD cpt-gearbox-fr-gear-category]", async ({
    studio,
  }) => {
    const groups = await studio.page.evaluate(() =>
      Array.from(document.querySelectorAll(".gbx-group-label")).map((e) =>
        (e.textContent ?? "").trim(),
      ),
    );
    expect(groups.length).toBeGreaterThan(1);
    // The seven values come from `gear.toml` files the platform team committed,
    // since folded into each `gear.gdl`.
    // "platform" is not one of them, and its appearance would mean the widget had
    // invented a bucket.
    expect(groups).not.toContain("platform");
    expect(groups).not.toContain("uncategorised");
  });
});
