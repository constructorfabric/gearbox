// What putting a newly scaffolded gear into a product actually writes.
//
// **Separated because a plugin is not a selected gear, and adding it as one is
// wrong twice over.** In the corpus a plugin appears only as a `plugin("id")`
// entry inside its host's `use_gear` -- never as a top-level `use_gear` of its
// own -- so `[add_source, add_gear]` leaves it attached to nothing, which the
// resolver rightly reports (GBX0518), *and* makes it a gear the product selected
// in its own right.
//
// Pure, and for the reason `shell/opening-outcome.ts` is: the widget that owns
// this decision cannot be constructed outside a browser, and the decision is the
// part worth checking. Writing to `products/` is what the conformance harness
// refuses, so a browser claim cannot reach the write either.

import type { ProductEdit } from "../../common/generated/ProductEdit";

/**
 * Where the host stands in the product this plugin is being added to.
 *
 * The engine's own refusal is what names these: attaching needs a `use_gear`
 * naming the host in `gears`, so "in the closure" is not the same as "named".
 */
export type HostStanding = "named" | "closure-only" | "absent";

export interface NewGearPlacement {
  readonly gearId: string;
  /** The source id the new folder is declared under. */
  readonly sourceId: string;
  /** Where that folder is, relative to the description. */
  readonly at: string;
  /**
   * Whether the gear being created is a plugin.
   *
   * **Separate from `host`, and conflating them was a hole.** `host === undefined`
   * meant two different things -- "an ordinary gear" and "a plugin whose host is
   * not decided yet" -- and both took the ordinary path, so a plugin with no host
   * got the top-level `use_gear` this very file says a plugin must never have.
   */
  readonly isPlugin: boolean;
  /**
   * The host this plugin plugs into, when one was chosen.
   */
  readonly host?: {
    readonly id: string;
    /** The source the *host* is read from, needed only to promote it. */
    readonly source: string;
    readonly standing: HostStanding;
  };
  /**
   * The profiles the connection applies under; absent or empty means every one.
   *
   * The wizard's third route to writing a connection, and it used to have no
   * scope at all: the Add Gear dialog's two routes carry one (ADR-0023,
   * 2026-09-21), and a plugin created for a product was attached under every
   * profile, beside whatever its host already runs there.
   */
  readonly profiles?: readonly string[];
}

export type Placement =
  | { readonly ok: true; readonly edits: readonly ProductEdit[] }
  | { readonly ok: false; readonly reason: string };

/**
 * The one batch that declares the folder and puts the gear where it belongs.
 *
 * One batch rather than a sequence, because `applyEdits` folds them onto the
 * same text and fails the whole thing on any refusal -- a half-added gear is the
 * state this wizard used to be able to produce.
 */
export function placeNewGear(placement: NewGearPlacement, productLabel: string): Placement {
  const declare: ProductEdit = {
    kind: "add_source",
    id: placement.sourceId,
    at: placement.at,
  };

  const host = placement.host;
  if (host === undefined) {
    if (placement.isPlugin) {
      // **A plugin with no host cannot be placed at all.** Not "added and left
      // for the resolver to complain about": the only form a plugin takes in a
      // description is an entry inside a host's `use_gear`, so there is no edit
      // to make. The wizard is where this is decided, which is why it refuses
      // here rather than writing something and apologising.
      return {
        ok: false,
        reason:
          `${placement.gearId} is a plugin, so it goes inside the gear it implements a point of. Choose which point it ` +
          `implements, or create it on its own and add it to a product later.`,
      };
    }
    return { ok: true, edits: [declare, select(placement.gearId, placement.sourceId)] };
  }

  if (host.standing === "absent") {
    // Adding the host is a decision about the product, not about this gear, so
    // it is refused with the name of the thing to do rather than done quietly.
    return {
      ok: false,
      reason:
        `${productLabel} does not use ${host.id}, which is the gear ${placement.gearId} plugs ` +
        `into. Add ${host.id} to the product first, then create this plugin.`,
    };
  }

  // A host the closure pulled in has no `use_gear` to attach to, so it is
  // promoted to an explicit one first. That is a visible change to the
  // description, which is why it is part of the previewed batch rather than a
  // side effect.
  const promote: readonly ProductEdit[] =
    host.standing === "closure-only" ? [select(host.id, host.source)] : [];

  return {
    ok: true,
    edits: [
      declare,
      ...promote,
      // **The plugin goes inside the host, and appends.** `set_plugins` would
      // rewrite the host's list as bare `plugin("id")` entries and drop the
      // `profiles` and `config` the other entries carry.
      placement.profiles === undefined || placement.profiles.length === 0
        ? { kind: "add_plugin", gear: host.id, plugin: placement.gearId }
        : {
            kind: "add_plugin_selection",
            gear: host.id,
            plugin: placement.gearId,
            profiles: [...placement.profiles],
          },
    ],
  };
}

function select(gear: string, source: string): ProductEdit {
  return { kind: "add_gear", gear, source };
}
