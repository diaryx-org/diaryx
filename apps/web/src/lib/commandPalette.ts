import type { CommandDefinition } from "$lib/commandRegistry";

export interface CommandPaletteGroup {
  key: CommandDefinition["group"];
  label: string;
  commands: CommandDefinition[];
}

const COMMAND_GROUPS = ["insert", "entry", "editor", "export", "workspace"] as const;

export function getFavoriteCommands(
  commandRegistry: Map<string, CommandDefinition>,
  favoriteIds: string[],
): CommandDefinition[] {
  return favoriteIds
    .map((id) => commandRegistry.get(id))
    .filter((cmd): cmd is CommandDefinition => !!cmd && cmd.available());
}

export function getGroupedCommands(
  commandRegistry: Map<string, CommandDefinition>,
): CommandPaletteGroup[] {
  return COMMAND_GROUPS
    .map((group) => ({
      key: group,
      label: group.charAt(0).toUpperCase() + group.slice(1),
      commands: [...commandRegistry.values()].filter(
        (command) => command.group === group && command.available(),
      ),
    }))
    .filter((group) => group.commands.length > 0);
}

export function reorderFavoriteIds(
  ids: string[],
  fromIndex: number,
  toIndex: number,
): string[] {
  if (
    fromIndex < 0 ||
    fromIndex >= ids.length ||
    toIndex < 0 ||
    toIndex >= ids.length ||
    fromIndex === toIndex
  ) {
    return ids;
  }

  const next = [...ids];
  const [item] = next.splice(fromIndex, 1);
  next.splice(toIndex, 0, item);
  return next;
}

export function shouldDismissPalette(dismissDragY: number, threshold = 80): boolean {
  return dismissDragY > threshold;
}
