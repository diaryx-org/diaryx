import { describe, expect, it, vi } from "vitest";

import type { CommandDefinition } from "$lib/commandRegistry";
import {
  getFavoriteCommands,
  getGroupedCommands,
  reorderFavoriteIds,
  shouldDismissPalette,
} from "./commandPalette";

function makeCommand(
  id: string,
  group: CommandDefinition["group"],
  available = true,
): CommandDefinition {
  return {
    id,
    label: id,
    group,
    icon: {} as never,
    available: () => available,
    execute: vi.fn(),
    favoritable: true,
  };
}

describe("commandPalette helpers", () => {
  it("keeps only existing available favorites in configured order", () => {
    const registry = new Map<string, CommandDefinition>([
      ["insert:a", makeCommand("insert:a", "insert")],
      ["workspace:b", makeCommand("workspace:b", "workspace", false)],
      ["entry:c", makeCommand("entry:c", "entry")],
    ]);

    expect(getFavoriteCommands(registry, ["entry:c", "missing", "workspace:b", "insert:a"]).map((cmd) => cmd.id))
      .toEqual(["entry:c", "insert:a"]);
  });

  it("groups available commands by palette section", () => {
    const registry = new Map<string, CommandDefinition>([
      ["workspace:b", makeCommand("workspace:b", "workspace")],
      ["insert:a", makeCommand("insert:a", "insert")],
      ["editor:c", makeCommand("editor:c", "editor", false)],
    ]);

    expect(getGroupedCommands(registry)).toEqual([
      expect.objectContaining({
        key: "insert",
        label: "Insert",
        commands: [expect.objectContaining({ id: "insert:a" })],
      }),
      expect.objectContaining({
        key: "workspace",
        label: "Workspace",
        commands: [expect.objectContaining({ id: "workspace:b" })],
      }),
    ]);
  });

  it("reorders favorites without mutating the input array", () => {
    const ids = ["a", "b", "c"];
    expect(reorderFavoriteIds(ids, 0, 2)).toEqual(["b", "c", "a"]);
    expect(ids).toEqual(["a", "b", "c"]);
    expect(reorderFavoriteIds(ids, -1, 2)).toBe(ids);
  });

  it("uses the dismiss threshold for mobile drag-to-close", () => {
    expect(shouldDismissPalette(81)).toBe(true);
    expect(shouldDismissPalette(80)).toBe(false);
    expect(shouldDismissPalette(50, 40)).toBe(true);
  });
});
