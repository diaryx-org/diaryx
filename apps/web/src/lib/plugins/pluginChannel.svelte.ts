export type PluginChannel = "stable" | "dev";

const STORAGE_KEY = "diaryx-plugin-channel";

let channelsState: Record<string, PluginChannel> = $state(loadChannels());

function loadChannels(): Record<string, PluginChannel> {
  if (typeof localStorage === "undefined") return {};

  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return {};
    const parsed = JSON.parse(raw);
    if (!parsed || typeof parsed !== "object") return {};
    return parsed as Record<string, PluginChannel>;
  } catch {
    return {};
  }
}

function saveChannels(channels: Record<string, PluginChannel>): void {
  if (typeof localStorage !== "undefined") {
    try {
      localStorage.setItem(STORAGE_KEY, JSON.stringify(channels));
    } catch {
      // Ignore storage write failures.
    }
  }

  channelsState = { ...channels };
}

export function getPluginChannels(): Record<string, PluginChannel> {
  return channelsState;
}

/** A plugin defaults to the stable channel until explicitly opted in. */
export function getPluginChannel(pluginId: string): PluginChannel {
  return channelsState[pluginId] ?? "stable";
}

export function setPluginChannel(
  pluginId: string,
  channel: PluginChannel,
): void {
  // "stable" is the default, so storing it is just noise — clear instead.
  if (channel === "stable") {
    clearPluginChannel(pluginId);
    return;
  }

  saveChannels({
    ...channelsState,
    [pluginId]: channel,
  });
}

export function clearPluginChannel(pluginId: string): void {
  if (!(pluginId in channelsState)) return;
  const next = { ...channelsState };
  delete next[pluginId];
  saveChannels(next);
}
