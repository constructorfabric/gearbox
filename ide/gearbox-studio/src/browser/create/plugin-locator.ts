// Whether a scaffolded plugin's `sdk` locator can be written, and if not, why.
//
// A pure module for the reason `paths.ts` and `gear-edits.ts` are: the widget
// that asks cannot be constructed outside a browser, and three of the four
// answers below are unreachable on this corpus and this platform -- one volume,
// and every SDK here sits under its own source root at a valid `RelPath`. They
// are checked in `store-smoke` instead of being asserted in prose.
//
// **The shape this replaces returned `PluginScaffold | undefined`, and
// `undefined` meant four unrelated things.** The consumer spread it as
// `...(scaffold === undefined ? {} : { plugin: scaffold })`, so a stale host
// key, an SDK the catalogue could not resolve, a relative destination and an SDK
// on another drive all silently dropped the whole field -- the engine then wrote
// the locator as a comment, and Create stayed enabled. A person was told by the
// picker that "the locator is written from this host" and got a plugin that
// cannot load. Distinguishing them by type is the fix; each arm is a different
// thing to say and a different thing to allow.

import type { GearDescriptor } from "../../common/generated/GearDescriptor";
import type { ExtensionPointDecl } from "../../common/generated/ExtensionPointDecl";
import type { PluginScaffold } from "../../common/generated/PluginScaffold";
import { specSegment } from "../../common/extension-points";
import { relativePath, volumeOf } from "./paths";

/**
 * What the panel may do about the locator.
 *
 * Four arms, and `none` is not a degenerate `blocked`. "I know it is a plugin
 * but not yet whose" is the state a person opens the picker in, it is what the
 * engine's commented `sdk` line is *for* (`PluginScaffold`, `protocol.rs`), and
 * putting a warning or a renamed button in front of it would be reporting a
 * problem that does not exist yet.
 *
 * `draft` is the one refusal that is a legitimate degradation rather than an
 * inconsistency: an SDK on another volume has no relative path from here on any
 * platform, so a commented locator is the honest answer and the crate is still
 * worth writing. `blocked` is the rest -- states where the panel and the session
 * disagree, and writing anything would put that disagreement on disk.
 */
export type LocatorOutcome =
  | { readonly kind: "none" }
  | { readonly kind: "ready"; readonly scaffold: PluginScaffold }
  | { readonly kind: "draft"; readonly reason: string }
  | { readonly kind: "blocked"; readonly reason: string };

/** A host's declared point, keyed the way the picker keys it. */
export interface HostPoint {
  /**
   * The picker's own option value, `hostId::TraitIdent`.
   *
   * Deliberately *not* `pointKey()`'s `sdk_lib::TraitIdent`: two hosts can
   * declare the same point, so the host has to be in the key for the picker to
   * address one of them, and a person reading a refusal recognises the gear id.
   */
  readonly key: string;
  readonly host: GearDescriptor;
  readonly point: ExtensionPointDecl;
}

export interface LocatorRequest {
  readonly isPlugin: boolean;
  /** The picker's value; `""` is "not decided yet". */
  readonly pointKey: string;
  /** Parent folder of the new gear, absolute; `""` when no folder is open. */
  readonly destinationDir: string;
  readonly gearId: string;
  readonly hosts: readonly HostPoint[];
  /** `CatalogueStore.absolutePath`, injected the way `sourceRootsOf` takes its resolver. */
  readonly absolutePath: (source: string, relative: string) => string | undefined;
}

/**
 * Whether the `sdk = cargo(...)` line can be written, and what to say if not.
 *
 * Ordered, because the causes are not independent: a stale key has no host to
 * ask about, and there is no arithmetic to do before there is a destination to
 * do it from.
 */
export function pluginLocatorFor(request: LocatorRequest): LocatorOutcome {
  const { isPlugin, pointKey, destinationDir, gearId, hosts, absolutePath } = request;
  if (!isPlugin || pointKey === "") return { kind: "none" };

  const chosen = hosts.find((entry) => entry.key === pointKey);
  if (chosen === undefined) {
    // The catalogue reloaded and no longer declares what was chosen. Nothing
    // here can be recovered by guessing another host, and scaffolding against a
    // point that is gone writes a plugin filling something nobody declares.
    return {
      kind: "blocked",
      reason:
        `The catalogue no longer declares ${pointKey}, so there is no SDK to point at. ` +
        `Choose a host again.`,
    };
  }

  if (destinationDir === "") {
    // **The fabricated-locator case.** With no folder open the destination is
    // empty, `${destinationDir}/${gearId}` is `/<gearId>`, and that *parses* as
    // a POSIX absolute path -- so nothing refused and a live locator was
    // computed relative to a root that does not exist. Said before any
    // arithmetic, because the arithmetic is what produces the plausible lie.
    return {
      kind: "blocked",
      reason:
        `No folder is open, so there is nowhere to write the gear and no way to say ` +
        `where the SDK sits relative to it. Open a folder, or choose a destination.`,
    };
  }

  // **The SDK's own crate.** This used to read `crate_name` and `path` off the
  // *host's* package, which for Authentication Resolver produced
  // `cf-gears-authn-resolver` beside `lib = "authn_resolver_sdk"` -- a host
  // crate wearing an SDK's library identifier -- at a path invented from a fixed
  // `../../`. The point carries the whole locator now.
  const sdk = chosen.point.sdk;
  const sdkAbsolute = absolutePath(chosen.host.source, sdk.path);
  if (sdkAbsolute === undefined) {
    // Two causes underneath -- an unknown source id, and a path that is not
    // catalogue-relative -- left undistinguished on purpose: `absolutePath` is
    // the one place that vets an engine-supplied relative path, telling them
    // apart means duplicating its rule out here, and there is one thing to do
    // about either.
    return {
      kind: "blocked",
      reason:
        `${chosen.host.id}'s SDK cannot be located: source ${chosen.host.source} does not ` +
        `resolve ${sdk.path}. Reload the catalogue, or check that source's root.`,
    };
  }

  // `path` in a `cargo(...)` is relative to the description's own directory, and
  // this description will live in `<destination>/<id>/`.
  const from = `${destinationDir}/${gearId}`;
  const path = relativePath(from, sdkAbsolute);
  if (path === undefined) {
    // `relativePath` stays the decision; `volumeOf` only *explains a refusal
    // that already happened*, so the explanation cannot disagree with the
    // decision and there is no second implementation of the volume rule.
    const here = volumeOf(from);
    if (here === undefined) {
      return {
        kind: "blocked",
        reason:
          `${destinationDir} is not an absolute path, so the SDK's location cannot be ` +
          `expressed relative to it. Give a full path, or use Choose….`,
      };
    }
    const there = volumeOf(sdkAbsolute);
    // Roots are reported as `volumeOf` folded them, since that is the identity
    // that was compared: `C:` and `c:` are one drive.
    return {
      kind: "draft",
      reason:
        `The destination is on ${here.root} and ${chosen.host.id}'s SDK is on ` +
        `${there?.root ?? "another volume"}, so no portable path can be written between them. ` +
        `The plugin declaration will be left as a comment.`,
    };
  }

  return {
    kind: "ready",
    scaffold: {
      // The declaration that makes the new gear a plugin: the spec the host
      // declares, as `implements` writes it.
      spec: specSegment(chosen.point.spec),
      trait_ident: chosen.point.trait_ident,
      crate_name: sdk.crate_name,
      lib_ident: sdk.lib_ident,
      path,
    },
  };
}
