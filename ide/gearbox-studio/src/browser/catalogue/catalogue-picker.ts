// The catalogue as something you call, not something that sits there.
//
// ADR-0011 made the catalogue a perspective, equal to the product. In practice
// nobody browses it except while composing a product, which is the case its own
// revisit clause named -- so it is a **source of components** now: a panel in
// Home, where looking at what exists is the only thing there is to do, and a
// picker in the product context, where the panel had been taking the whole left
// side while the product sat in a secondary tab.
//
// **Adding is a write through the Add Gear configurator.** Finding a gear selects
// it, which fills the Inspector -- what it is, and, if the product names it, why it
// is here. The catalogue panel's `+` opens that configurator; removal still uses
// the panel check with its preview and confirmation.
//
// The quick-pick rather than a dialog because Theia's quick input is what people
// already type into, and because a filter over fourteen rows -- or four hundred --
// is a search box, not a tree.

import { QuickInputService, QuickPickItem } from "@theia/core/lib/common/quick-pick-service";
import { Command, CommandContribution, CommandRegistry } from "@theia/core/lib/common/command";
import { MenuContribution, MenuModelRegistry } from "@theia/core/lib/common/menu";
import { codicon } from "@theia/core/lib/browser";
import { inject, injectable } from "@theia/core/shared/inversify";

import { rowKey, rowName, type Row } from "../../common/protocol";
import { CatalogueStore } from "../catalogue-store";
import { VIEW_CATALOGUE } from "../menus";
import { SelectionService } from "../shell/selection-service";

export const FIND_GEAR: Command = {
  id: "gearbox.catalogue.find",
  label: "Gearbox: Find Gear…",
  shortTitle: "Find Gear…",
  iconClass: codicon("search"),
};

/** A quick-pick item that remembers which row it came from. */
interface GearItem extends QuickPickItem {
  readonly key: string;
}

@injectable()
export class CataloguePicker implements CommandContribution, MenuContribution {
  @inject(CatalogueStore) protected readonly catalogue!: CatalogueStore;
  @inject(SelectionService) protected readonly selection!: SelectionService;
  @inject(QuickInputService) protected readonly quickInput!: QuickInputService;
  @inject(CommandRegistry) protected readonly commands!: CommandRegistry;

  registerCommands(commands: CommandRegistry): void {
    commands.registerCommand(FIND_GEAR, {
      execute: () => this.pick(),
      // Enabled only once there is something to find. A picker that opens on an
      // empty catalogue is a question with no answers, and the catalogue loads in
      // stages, so "empty" is a real state for the first second of a session.
      isEnabled: () => this.catalogue.current.rows.length > 0,
    });
  }

  registerMenus(menus: MenuModelRegistry): void {
    menus.registerMenuAction(VIEW_CATALOGUE, {
      commandId: FIND_GEAR.id,
      label: "Find Gear…",
      order: "1",
    });
  }

  /**
   * Offer every gear, and select the one chosen.
   *
   * Sorted by name rather than grouped by category. The panel groups, because a
   * tree is for browsing a shape; a picker is for finding a name you already have
   * in mind, and a category header between the two candidates you are choosing
   * between is in the way.
   */
  protected async pick(): Promise<void> {
    const rows = [...this.catalogue.current.rows].sort((a, b) =>
      rowName(a).localeCompare(rowName(b)),
    );
    const items: GearItem[] = rows.map((row) => ({
      label: rowName(row),
      description: describe(row),
      key: rowKey(row),
    }));

    const chosen = await this.quickInput.showQuickPick(items, {
      placeholder: "Find a gear by name, id or category",
      // Matches the description too, so typing `transport` finds the gears in
      // that category and typing `types-registry` finds it by id -- neither of
      // which is the label.
      matchOnDescription: true,
    });
    if (chosen === undefined) return;

    // Through the store, so a pending row becomes a row selection and a projected
    // one becomes a gear selection. That normalisation is what makes this the same
    // selection as one made in the product tree.
    this.catalogue.select(chosen.key);
    // The panel is collapsed in the product context, so the answer has to appear
    // somewhere visible: the Inspector is where a selection is explained.
    await this.commands.executeCommand("gearbox.inspector.toggle");
  }
}

/**
 * The line under the name: the id and the category, when there are any.
 *
 * A pending row has no id -- it does not exist until S2 has projected the crate
 * (ADR `cpt-gearbox-adr-staged-catalogue-loading`) -- so it says so instead of
 * showing a blank where every other row has an identifier.
 */
function describe(row: Row): string {
  if (row.kind === "pending") {
    const category = row.gear.category ?? undefined;
    return category === undefined ? "not parsed yet" : `${category} · not parsed yet`;
  }
  const parts = [row.gear.id];
  if (row.gear.category !== null && row.gear.category !== undefined) {
    parts.push(row.gear.category);
  }
  if (row.kind === "design") parts.push("design, no code yet");
  return parts.join(" · ");
}
