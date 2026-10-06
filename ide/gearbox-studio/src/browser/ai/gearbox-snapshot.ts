// The small, true description of what Studio is looking at.
//
// **Small is the requirement, not a nicety.** A context window spent on the
// catalogue is a context window not spent on the question, and the catalogue is
// the largest thing here: eighty-one diagnostic codes, a resolution with every
// gear's config, a graph with an edge per decision. So each of these returns
// identifiers and counts, and the tools fetch detail for the few things a
// question turns out to be about.
//
// **Every list says when it truncated.** A silently cut list is worse than a
// short one, because the model cannot tell the difference between "there are
// three" and "here are three of forty" -- and will happily conclude the former.
//
// Pure functions over the stores rather than a service: there is no state here,
// and a caller that has the stores has everything. That also makes each of these
// answerable in a test without a container.

import type { Diagnostic } from "../../common/generated";
import type { CatalogueState, Row } from "../../common/protocol";
import { rowCategory, rowName } from "../../common/protocol";
import type { ProductState } from "../product-store";
import type { Selection } from "../shell/selection-service";

/** How many items any one list hands over before it starts counting instead. */
export const LIST_CAP = 40;

/** A list that knows what it left out. */
export interface Capped<T> {
  readonly items: readonly T[];
  /** How many there were in total, which may exceed `items.length`. */
  readonly total: number;
  /** How many are not in `items`. Zero when the list is complete. */
  readonly omitted: number;
}

export function cap<T>(all: readonly T[], limit = LIST_CAP): Capped<T> {
  const items = all.slice(0, Math.max(0, limit));
  return { items, total: all.length, omitted: all.length - items.length };
}

/**
 * What is selected, in the `NodeId` vocabulary.
 *
 * `catalogue-row` is carried through rather than resolved to a gear: before S2
 * has projected the row there is no `GearId` to resolve it to (ADR-0009), and
 * inventing one would be inventing the answer.
 */
export type SelectionSnapshot =
  | Extract<Selection, { kind: "plugin" }>
  | { readonly kind: "gear"; readonly id: string; readonly label?: string }
  | { readonly kind: "application"; readonly id: string }
  | { readonly kind: "binding"; readonly consumer: string; readonly contract: string }
  | { readonly kind: "catalogue-row"; readonly key: string; readonly label?: string }
  | { readonly kind: "none" };

export function selectionSnapshot(
  selection: Selection | undefined,
  rowOf: (key: string) => Row | undefined,
): SelectionSnapshot {
  if (selection === undefined) return { kind: "none" };
  switch (selection.kind) {
    case "plugin": return selection;
    // **Both acts snapshot as one gear.** The chat is being asked about the
    // gear, and which panel the person clicked is not part of the question. The
    // distinction exists to decide what may be *edited*, and nothing here edits.
    case "gear":
    case "catalogue-gear":
      return { kind: "gear", id: selection.id };
    case "application":
      return { kind: "application", id: selection.id };
    case "binding":
      return { kind: "binding", consumer: selection.consumer, contract: selection.contract };
    case "catalogue-row": {
      const row = rowOf(selection.key);
      return {
        kind: "catalogue-row",
        key: selection.key,
        ...(row === undefined ? {} : { label: rowName(row) }),
      };
    }
  }
}

/** A one-line label for a selection, for the chip a person reads. */
export function selectionLabel(
  selection: Selection | undefined,
  rowOf: (key: string) => Row | undefined,
): string {
  if (selection === undefined) return "nothing selected";
  switch (selection.kind) {
    case "plugin":
      return `${selection.host} / ${selection.id} (connection ${selection.entryIndex + 1})`;
    case "gear":
    case "catalogue-gear":
      return selection.id;
    case "application":
      return selection.id;
    case "binding":
      return `${selection.consumer} → ${selection.contract}`;
    case "catalogue-row": {
      const row = rowOf(selection.key);
      return row === undefined ? selection.key : rowName(row);
    }
  }
}

/** Identity and provenance of whatever product is open. */
export interface ProductSnapshot {
  readonly product: string | undefined;
  readonly path: string | undefined;
  readonly profile: string | undefined;
  readonly profile_kind?: string;
  /** Which directory generated application crates go under. */
  readonly layout?: string;
  /**
   * The lock hash, which is the resolution's durable identity.
   *
   * Not `ProductStore.revision`: that is a frontend epoch counter for cache
   * invalidation, meaningless outside this tab and across a reload.
   */
  readonly revision?: string;
  readonly gearbox_version?: string;
  /** Absent while nothing has resolved yet, which is not the same as an error. */
  readonly resolved: boolean;
}

export function productSnapshot(state: ProductState): ProductSnapshot {
  const header = state.resolution?.product?.product;
  return {
    product: state.intent?.id ?? state.open?.label,
    path: state.open?.path,
    profile: state.profile,
    ...(header === undefined
      ? {}
      : {
          profile_kind: header.profile_kind,
          layout: header.layout,
          revision: header.lock_hash,
          gearbox_version: header.gearbox_version,
        }),
    resolved: state.resolution?.product != null,
  };
}

/** One diagnostic, flattened to what a question about it needs. */
export interface DiagnosticSnapshot {
  readonly code: string;
  readonly severity: string;
  readonly message: string;
  readonly subject?: string;
  readonly help?: string;
  readonly at?: string;
}

function diagnosticSnapshot(diagnostic: Diagnostic): DiagnosticSnapshot {
  const location = diagnostic.location;
  return {
    code: diagnostic.code,
    severity: diagnostic.severity,
    message: diagnostic.message,
    ...(diagnostic.subject == null ? {} : { subject: diagnostic.subject }),
    ...(diagnostic.help == null ? {} : { help: diagnostic.help }),
    // `uri` is a `file://` URI and the line is zero-based on the wire; a
    // person and a model both count from one, and both want the file's name
    // rather than its whole path.
    ...(location == null
      ? {}
      : {
          at: `${location.uri.split("/").pop() ?? location.uri}:${
            location.range.start.line + 1
          }`,
        }),
  };
}

/**
 * Everything currently complained about, resolution and catalogue together.
 *
 * Both, because "why is this not in dev" can be answered by either: a gear
 * missing from a resolution and a gear whose description failed to project are
 * the same question from the operator's side, and splitting them across two
 * tools would make the model pick, and pick wrong.
 */
export function diagnosticsSnapshot(
  product: ProductState,
  catalogue: CatalogueState,
  limit = LIST_CAP,
): Capped<DiagnosticSnapshot> & { readonly errors: number } {
  const all = [...product.diagnostics, ...catalogue.diagnostics];
  const errors = all.filter((d) => d.severity === "error").length;
  // Worst first, so a cap keeps what matters rather than what sorted first by
  // code. `Diagnostics` arrives ordered by `(code, message)`, which is a
  // reference order, not a triage order.
  const rank: Record<string, number> = { error: 0, warning: 1, info: 2, hint: 3 };
  const sorted = [...all].sort((a, b) => (rank[a.severity] ?? 9) - (rank[b.severity] ?? 9));
  const capped = cap(sorted.map(diagnosticSnapshot), limit);
  return { ...capped, errors };
}

/** The resolved topology, as ids. */
export interface TopologySnapshot {
  readonly applications: readonly {
    readonly name: string;
    readonly kind: string;
    readonly anchor: string;
    readonly replicas: number;
    readonly gears: readonly string[];
  }[];
  readonly bindings: readonly {
    readonly consumer: string;
    readonly contract: string;
    readonly provider: string;
    readonly mode: string;
  }[];
  readonly gearCount: number;
  readonly omittedApplications: number;
}

export function topologySnapshot(
  state: ProductState,
  limit = LIST_CAP,
): TopologySnapshot | undefined {
  const resolved = state.resolution?.product;
  if (resolved == null) return undefined;
  const applications = cap(resolved.applications, limit);
  return {
    applications: applications.items.map((application) => ({
      name: application.name,
      kind: application.kind,
      anchor: application.anchor,
      replicas: application.replicas,
      gears: application.gears,
    })),
    bindings: (resolved.bindings ?? []).slice(0, limit).map((binding) => ({
      consumer: binding.consumer,
      contract: binding.contract,
      provider: binding.provider,
      mode: String(binding.mode),
    })),
    gearCount: Object.keys(resolved.gears ?? {}).length,
    omittedApplications: applications.omitted,
  };
}

/** A catalogue row, as a list entry rather than a full descriptor. */
export interface GearListEntry {
  readonly id: string;
  readonly name: string;
  readonly category: string;
  /** Pending rows have no `GearId` yet (ADR-0009), so they are named by key. */
  readonly pending: boolean;
  /**
   * Described at design maturity: no crate, so not a gear to add. Present
   * only when true, so the common entry stays as short as it was.
   */
  readonly design?: true;
  /**
   * Named as a top-level gear by the description.
   *
   * Spelled the same as `gearbox_get_gear`'s field on purpose: a plugin is
   * never selected directly, so a name like `inProduct` invites the reader to
   * conclude the description does not mention it at all.
   */
  readonly selectedDirectly: boolean;
}

export function gearList(
  rows: readonly Row[],
  inProduct: (gear: string) => boolean,
  filter: string | undefined,
  limit = LIST_CAP,
): Capped<GearListEntry> {
  const needle = filter?.trim().toLowerCase();
  const entries = rows
    .map((row): GearListEntry => {
      const pending = row.kind === "pending";
      const id = pending ? rowName(row) : row.gear.id;
      return {
        id,
        name: rowName(row),
        category: rowCategory(row),
        pending,
        ...(row.kind === "design" ? { design: true as const } : {}),
        selectedDirectly: !pending && inProduct(id),
      };
    })
    .filter(
      (entry) =>
        needle === undefined ||
        needle === "" ||
        entry.id.toLowerCase().includes(needle) ||
        entry.name.toLowerCase().includes(needle) ||
        entry.category.toLowerCase().includes(needle),
    );
  return cap(entries, limit);
}
