/**
 * Appearance store — manages theme presets, typography presets, accent hue,
 * and typography/layout overrides.
 *
 * Orthogonal to the theme (light/dark/system) store. A user picks a color
 * theme preset AND a typography preset independently.
 */

import type {
  ContentWidth,
  FontFamily,
  ThemeCatalogEntry,
  ThemeDefinition,
  ThemeExport,
  ThemeLibraryEntry,
  ThemeSourceMetadata,
  TypographyCatalogEntry,
  TypographyDefinition,
  TypographyExport,
  TypographyLibraryEntry,
  TypographySettings,
  TypographySourceMetadata,
  UserAppearance,
} from "./appearance.types";
import {
  BUILTIN_PRESETS,
  BUILTIN_PRESET_LIST,
  BUILTIN_TYPOGRAPHY_PRESETS,
  BUILTIN_TYPOGRAPHY_PRESET_LIST,
} from "./appearance.presets";
import {
  DEFAULT_APPEARANCE,
  applyCssVars,
  cacheVarsForFouc,
  clearCssVars,
  clearVarsCache,
  resolveEffectivePalette,
} from "./appearance.utils";
import { getThemeStore } from "./theme.svelte";
import {
  getThemeLibraryPath,
  getThemeSettingsPath,
  getTypographyLibraryPath,
  getTypographySettingsPath,
  readWorkspaceText,
  writeWorkspaceText,
} from "$lib/workspace/workspaceAssetStorage";

const APPEARANCE_STORAGE_KEY = "diaryx-appearance";
const THEME_LIBRARY_STORAGE_KEY = "diaryx-theme-library-v1";
const TYPOGRAPHY_LIBRARY_STORAGE_KEY = "diaryx-typography-library-v1";

function cloneDefaultAppearance(): UserAppearance {
  return {
    ...DEFAULT_APPEARANCE,
    typographyOverrides: { ...DEFAULT_APPEARANCE.typographyOverrides },
  };
}

function cloneTheme(theme: ThemeDefinition): ThemeDefinition {
  return {
    ...theme,
    colors: {
      light: { ...theme.colors.light },
      dark: { ...theme.colors.dark },
    },
  };
}

function cloneTypographySettings(
  settings: TypographySettings,
): TypographySettings {
  return {
    fontFamily: settings.fontFamily,
    baseFontSize: settings.baseFontSize,
    lineHeight: settings.lineHeight,
    contentWidth: settings.contentWidth,
  };
}

function cloneTypography(
  typography: TypographyDefinition,
): TypographyDefinition {
  return {
    ...typography,
    settings: cloneTypographySettings(typography.settings),
  };
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function isFiniteNumber(value: unknown): value is number {
  return typeof value === "number" && Number.isFinite(value);
}

function isFontFamily(value: unknown): value is FontFamily {
  return (
    value === "inter" ||
    value === "system" ||
    value === "serif" ||
    value === "mono"
  );
}

function isContentWidth(value: unknown): value is ContentWidth {
  return (
    value === "narrow" ||
    value === "medium" ||
    value === "wide" ||
    value === "full"
  );
}

function isThemeDefinition(value: unknown): value is ThemeDefinition {
  if (!isRecord(value)) return false;
  if (typeof value.id !== "string" || value.id.length === 0) return false;
  if (typeof value.name !== "string" || value.name.length === 0) return false;
  if (value.version !== 1) return false;
  if (!isRecord(value.colors)) return false;
  if (!isRecord(value.colors.light) || !isRecord(value.colors.dark)) return false;
  return true;
}

function isTypographyDefinition(value: unknown): value is TypographyDefinition {
  if (!isRecord(value)) return false;
  if (typeof value.id !== "string" || value.id.length === 0) return false;
  if (typeof value.name !== "string" || value.name.length === 0) return false;
  if (value.version !== 1) return false;
  if (!isRecord(value.settings)) return false;
  return (
    isFontFamily(value.settings.fontFamily) &&
    isFiniteNumber(value.settings.baseFontSize) &&
    isFiniteNumber(value.settings.lineHeight) &&
    isContentWidth(value.settings.contentWidth)
  );
}

function parseTypographySettingsInput(
  value: unknown,
): Partial<TypographySettings> {
  if (!isRecord(value)) return {};

  const out: Partial<TypographySettings> = {};

  if (isFontFamily(value.fontFamily)) {
    out.fontFamily = value.fontFamily;
  }
  if (isFiniteNumber(value.baseFontSize)) {
    out.baseFontSize = value.baseFontSize;
  }
  if (isFiniteNumber(value.lineHeight)) {
    out.lineHeight = value.lineHeight;
  }
  if (isContentWidth(value.contentWidth)) {
    out.contentWidth = value.contentWidth;
  }

  return out;
}

function normalizeTypographyOverrides(
  overrides: Partial<TypographySettings>,
  preset: TypographySettings,
): Partial<TypographySettings> {
  const next = parseTypographySettingsInput(overrides);
  const out: Partial<TypographySettings> = {};

  if (next.fontFamily && next.fontFamily !== preset.fontFamily) {
    out.fontFamily = next.fontFamily;
  }
  if (
    typeof next.baseFontSize === "number" &&
    next.baseFontSize !== preset.baseFontSize
  ) {
    out.baseFontSize = next.baseFontSize;
  }
  if (typeof next.lineHeight === "number" && next.lineHeight !== preset.lineHeight) {
    out.lineHeight = next.lineHeight;
  }
  if (next.contentWidth && next.contentWidth !== preset.contentWidth) {
    out.contentWidth = next.contentWidth;
  }

  return out;
}

function toTypographyOverrides(
  settings: Partial<TypographySettings>,
  preset: TypographySettings,
): Partial<TypographySettings> {
  return normalizeTypographyOverrides(settings, preset);
}

function readLegacyTypographySettings(
  input: Record<string, unknown>,
): Partial<TypographySettings> | null {
  const out: Partial<TypographySettings> = {};

  if (isRecord(input.typography)) {
    if (isFontFamily(input.typography.fontFamily)) {
      out.fontFamily = input.typography.fontFamily;
    }
    if (isFiniteNumber(input.typography.baseFontSize)) {
      out.baseFontSize = input.typography.baseFontSize;
    }
    if (isFiniteNumber(input.typography.lineHeight)) {
      out.lineHeight = input.typography.lineHeight;
    }
  }

  if (isRecord(input.layout) && isContentWidth(input.layout.contentWidth)) {
    out.contentWidth = input.layout.contentWidth;
  }

  return Object.keys(out).length > 0 ? out : null;
}

function loadAppearance(): UserAppearance {
  if (typeof window === "undefined") return cloneDefaultAppearance();

  try {
    const raw = localStorage.getItem(APPEARANCE_STORAGE_KEY);
    if (!raw) return migrateFromLegacy();

    const parsed = JSON.parse(raw);
    if (!isRecord(parsed)) {
      return cloneDefaultAppearance();
    }

    const next = cloneDefaultAppearance();

    if (typeof parsed.presetId === "string" && parsed.presetId.length > 0) {
      next.presetId = parsed.presetId;
    }

    if (isFiniteNumber(parsed.accentHue)) {
      next.accentHue = parsed.accentHue;
    } else if (parsed.accentHue === null) {
      next.accentHue = null;
    }

    if (
      typeof parsed.typographyPresetId === "string" &&
      parsed.typographyPresetId.length > 0
    ) {
      next.typographyPresetId = parsed.typographyPresetId;
    }

    const explicitOverrides = parseTypographySettingsInput(parsed.typographyOverrides);
    const legacyTypography = readLegacyTypographySettings(parsed);

    if (legacyTypography) {
      const preset =
        BUILTIN_TYPOGRAPHY_PRESETS[next.typographyPresetId] ??
        BUILTIN_TYPOGRAPHY_PRESETS.default;
      next.typographyOverrides = {
        ...toTypographyOverrides(legacyTypography, preset.settings),
        ...explicitOverrides,
      };
    } else {
      next.typographyOverrides = explicitOverrides;
    }

    return next;
  } catch {
    return cloneDefaultAppearance();
  }
}

function saveAppearance(appearance: UserAppearance): void {
  if (typeof window === "undefined") return;

  try {
    localStorage.setItem(APPEARANCE_STORAGE_KEY, JSON.stringify(appearance));
  } catch {
    // Ignore storage write errors.
  }
}

function parseThemeLibraryInput(input: unknown): Record<string, ThemeLibraryEntry> {
  if (!Array.isArray(input)) return {};

  const out: Record<string, ThemeLibraryEntry> = {};
  for (const item of input) {
    if (!isRecord(item) || !isThemeDefinition(item.theme)) continue;
    const sourceRaw = isRecord(item.source) ? item.source : {};
    const source: ThemeSourceMetadata = {
      source:
        sourceRaw.source === "registry" ||
        sourceRaw.source === "local" ||
        sourceRaw.source === "bundle"
          ? sourceRaw.source
          : "local",
      registryId:
        typeof sourceRaw.registryId === "string" ? sourceRaw.registryId : undefined,
      fileName:
        typeof sourceRaw.fileName === "string" ? sourceRaw.fileName : undefined,
      installedAt:
        typeof sourceRaw.installedAt === "number"
          ? sourceRaw.installedAt
          : undefined,
    };

    const theme = cloneTheme(item.theme);
    out[theme.id] = { theme, source };
  }

  return out;
}

function loadThemeLibrary(): Record<string, ThemeLibraryEntry> {
  if (typeof window === "undefined") return {};

  try {
    const raw = localStorage.getItem(THEME_LIBRARY_STORAGE_KEY);
    if (!raw) return {};
    return parseThemeLibraryInput(JSON.parse(raw));
  } catch {
    return {};
  }
}

function saveThemeLibrary(library: Record<string, ThemeLibraryEntry>): void {
  if (typeof window === "undefined") return;

  try {
    localStorage.setItem(
      THEME_LIBRARY_STORAGE_KEY,
      JSON.stringify(Object.values(library)),
    );
  } catch {
    // Ignore storage write errors.
  }
}

function parseTypographyLibraryInput(
  input: unknown,
): Record<string, TypographyLibraryEntry> {
  if (!Array.isArray(input)) return {};

  const out: Record<string, TypographyLibraryEntry> = {};
  for (const item of input) {
    if (!isRecord(item) || !isTypographyDefinition(item.typography)) continue;
    const sourceRaw = isRecord(item.source) ? item.source : {};
    const source: TypographySourceMetadata = {
      source:
        sourceRaw.source === "registry" ||
        sourceRaw.source === "local" ||
        sourceRaw.source === "bundle"
          ? sourceRaw.source
          : "local",
      registryId:
        typeof sourceRaw.registryId === "string" ? sourceRaw.registryId : undefined,
      fileName:
        typeof sourceRaw.fileName === "string" ? sourceRaw.fileName : undefined,
      installedAt:
        typeof sourceRaw.installedAt === "number"
          ? sourceRaw.installedAt
          : undefined,
    };

    const typography = cloneTypography(item.typography);
    out[typography.id] = { typography, source };
  }

  return out;
}

function loadTypographyLibrary(): Record<string, TypographyLibraryEntry> {
  if (typeof window === "undefined") return {};

  try {
    const raw = localStorage.getItem(TYPOGRAPHY_LIBRARY_STORAGE_KEY);
    if (!raw) return {};
    return parseTypographyLibraryInput(JSON.parse(raw));
  } catch {
    return {};
  }
}

function saveTypographyLibrary(
  library: Record<string, TypographyLibraryEntry>,
): void {
  if (typeof window === "undefined") return;

  try {
    localStorage.setItem(
      TYPOGRAPHY_LIBRARY_STORAGE_KEY,
      JSON.stringify(Object.values(library)),
    );
  } catch {
    // Ignore storage write errors.
  }
}

function parseThemeSettingsInput(input: unknown): Partial<UserAppearance> {
  if (!isRecord(input)) return {};

  const next: Partial<UserAppearance> = {};
  if (typeof input.presetId === "string" && input.presetId.length > 0) {
    next.presetId = input.presetId;
  }
  if (isFiniteNumber(input.accentHue)) {
    next.accentHue = input.accentHue;
  } else if (input.accentHue === null) {
    next.accentHue = null;
  }
  return next;
}

function parseTypographySettingsFileInput(
  input: unknown,
): Partial<UserAppearance> {
  if (!isRecord(input)) return {};

  const next: Partial<UserAppearance> = {};
  if (
    typeof input.typographyPresetId === "string" &&
    input.typographyPresetId.length > 0
  ) {
    next.typographyPresetId = input.typographyPresetId;
  }
  next.typographyOverrides = parseTypographySettingsInput(input.typographyOverrides);
  return next;
}

async function persistThemeWorkspaceFiles(
  appearance: UserAppearance,
  library: Record<string, ThemeLibraryEntry>,
): Promise<void> {
  try {
    await Promise.all([
      writeWorkspaceText(
        getThemeSettingsPath(),
        JSON.stringify(
          {
            presetId: appearance.presetId,
            accentHue: appearance.accentHue,
          },
          null,
          2,
        ),
      ),
      writeWorkspaceText(
        getThemeLibraryPath(),
        JSON.stringify(Object.values(library), null, 2),
      ),
    ]);
  } catch (error) {
    console.warn("[appearanceStore] Failed to persist workspace themes:", error);
  }
}

async function persistTypographyWorkspaceFiles(
  appearance: UserAppearance,
  library: Record<string, TypographyLibraryEntry>,
): Promise<void> {
  try {
    await Promise.all([
      writeWorkspaceText(
        getTypographySettingsPath(),
        JSON.stringify(
          {
            typographyPresetId: appearance.typographyPresetId,
            typographyOverrides: appearance.typographyOverrides,
          },
          null,
          2,
        ),
      ),
      writeWorkspaceText(
        getTypographyLibraryPath(),
        JSON.stringify(Object.values(library), null, 2),
      ),
    ]);
  } catch (error) {
    console.warn(
      "[appearanceStore] Failed to persist workspace typographies:",
      error,
    );
  }
}

function mergeThemeMap(
  library: Record<string, ThemeLibraryEntry>,
): Record<string, ThemeDefinition> {
  const merged: Record<string, ThemeDefinition> = {
    ...BUILTIN_PRESETS,
  };

  for (const entry of Object.values(library)) {
    merged[entry.theme.id] = entry.theme;
  }

  return merged;
}

function mergeTypographyMap(
  library: Record<string, TypographyLibraryEntry>,
): Record<string, TypographyDefinition> {
  const merged: Record<string, TypographyDefinition> = {
    ...BUILTIN_TYPOGRAPHY_PRESETS,
  };

  for (const entry of Object.values(library)) {
    merged[entry.typography.id] = entry.typography;
  }

  return merged;
}

function normalizeAppearance(
  appearance: UserAppearance,
  themeMap: Record<string, ThemeDefinition>,
  typographyMap: Record<string, TypographyDefinition>,
): UserAppearance {
  const next = {
    ...cloneDefaultAppearance(),
    ...appearance,
    typographyOverrides: parseTypographySettingsInput(appearance.typographyOverrides),
  };

  if (!themeMap[next.presetId]) {
    next.presetId = "default";
  }

  if (!typographyMap[next.typographyPresetId]) {
    next.typographyPresetId = "default";
  }

  if (next.accentHue !== null && !isFiniteNumber(next.accentHue)) {
    next.accentHue = null;
  }

  const typographyPreset =
    typographyMap[next.typographyPresetId] ?? BUILTIN_TYPOGRAPHY_PRESETS.default;
  next.typographyOverrides = normalizeTypographyOverrides(
    next.typographyOverrides,
    typographyPreset.settings,
  );

  return next;
}

function resolveTypographySettings(
  appearance: UserAppearance,
  typographyMap: Record<string, TypographyDefinition>,
): TypographySettings {
  const preset =
    typographyMap[appearance.typographyPresetId] ?? BUILTIN_TYPOGRAPHY_PRESETS.default;

  return {
    ...preset.settings,
    ...appearance.typographyOverrides,
  };
}

/** Migrate from legacy `readableLineLength` behavior. */
function migrateFromLegacy(): UserAppearance {
  const appearance = cloneDefaultAppearance();
  if (typeof window === "undefined") return appearance;

  const legacy = localStorage.getItem("diaryx-readable-line-length");
  if (legacy === "false") {
    appearance.typographyOverrides = { contentWidth: "full" };
  }

  return appearance;
}

export function createAppearanceStore() {
  const initialThemeLibrary = loadThemeLibrary();
  const initialTypographyLibrary = loadTypographyLibrary();

  let themeLibrary = $state<Record<string, ThemeLibraryEntry>>(initialThemeLibrary);
  let typographyLibrary = $state<Record<string, TypographyLibraryEntry>>(
    initialTypographyLibrary,
  );

  let appearance = $state<UserAppearance>(
    normalizeAppearance(
      loadAppearance(),
      mergeThemeMap(initialThemeLibrary),
      mergeTypographyMap(initialTypographyLibrary),
    ),
  );

  function getThemeMap(): Record<string, ThemeDefinition> {
    return mergeThemeMap(themeLibrary);
  }

  function getTypographyMap(): Record<string, TypographyDefinition> {
    return mergeTypographyMap(typographyLibrary);
  }

  function getThemeById(themeId: string): ThemeDefinition | undefined {
    return getThemeMap()[themeId];
  }

  function getTypographyById(
    typographyId: string,
  ): TypographyDefinition | undefined {
    return getTypographyMap()[typographyId];
  }

  function listThemes(): ThemeCatalogEntry[] {
    const builtins: ThemeCatalogEntry[] = BUILTIN_PRESET_LIST.map((theme) => ({
      theme,
      source: { source: "builtin" },
      builtin: true,
    }));

    const installed = Object.values(themeLibrary)
      .map((entry) => ({
        theme: entry.theme,
        source: entry.source,
        builtin: false,
      }))
      .sort((a, b) => a.theme.name.localeCompare(b.theme.name));

    return [...builtins, ...installed];
  }

  function listTypographies(): TypographyCatalogEntry[] {
    const builtins: TypographyCatalogEntry[] = BUILTIN_TYPOGRAPHY_PRESET_LIST.map(
      (typography) => ({
        typography,
        source: { source: "builtin" },
        builtin: true,
      }),
    );

    const installed = Object.values(typographyLibrary)
      .map((entry) => ({
        typography: entry.typography,
        source: entry.source,
        builtin: false,
      }))
      .sort((a, b) => a.typography.name.localeCompare(b.typography.name));

    return [...builtins, ...installed];
  }

  function apply() {
    if (typeof document === "undefined") return;

    const preset = getThemeById(appearance.presetId) ?? BUILTIN_PRESETS.default;
    const typography = resolveTypographySettings(appearance, getTypographyMap());

    const themeStore = getThemeStore();
    const mode = themeStore.resolvedTheme;

    const isDefault =
      appearance.presetId === "default" &&
      appearance.accentHue === null &&
      appearance.typographyPresetId === "default" &&
      Object.keys(appearance.typographyOverrides).length === 0;

    if (isDefault) {
      clearCssVars();
      clearVarsCache();
      return;
    }

    const palette = resolveEffectivePalette(preset, mode, appearance.accentHue);
    const vars = applyCssVars(palette, typography);
    cacheVarsForFouc(vars);
  }

  function update(partial: Partial<UserAppearance>): void {
    appearance = normalizeAppearance(
      {
        ...appearance,
        ...partial,
        typographyOverrides:
          partial.typographyOverrides ?? appearance.typographyOverrides,
      },
      getThemeMap(),
      getTypographyMap(),
    );

    saveAppearance(appearance);
    void persistThemeWorkspaceFiles(appearance, themeLibrary);
    void persistTypographyWorkspaceFiles(appearance, typographyLibrary);
    apply();
  }

  function isBuiltinTheme(themeId: string): boolean {
    return !!BUILTIN_PRESETS[themeId];
  }

  function installTheme(
    theme: ThemeDefinition,
    sourceMetadata: ThemeSourceMetadata = { source: "local" },
  ): boolean {
    if (!isThemeDefinition(theme) || theme.version !== 1) {
      return false;
    }

    if (isBuiltinTheme(theme.id)) {
      // Built-ins are immutable and always available.
      return true;
    }

    const source: ThemeSourceMetadata = {
      ...sourceMetadata,
      source: sourceMetadata.source ?? "local",
      installedAt: sourceMetadata.installedAt ?? Date.now(),
    };

    themeLibrary = {
      ...themeLibrary,
      [theme.id]: {
        theme: cloneTheme(theme),
        source,
      },
    };
    saveThemeLibrary(themeLibrary);
    void persistThemeWorkspaceFiles(appearance, themeLibrary);

    appearance = normalizeAppearance(appearance, getThemeMap(), getTypographyMap());
    saveAppearance(appearance);
    void persistTypographyWorkspaceFiles(appearance, typographyLibrary);
    apply();

    return true;
  }

  function uninstallTheme(themeId: string): boolean {
    if (isBuiltinTheme(themeId)) return false;
    if (!themeLibrary[themeId]) return false;

    const nextLibrary = { ...themeLibrary };
    delete nextLibrary[themeId];
    themeLibrary = nextLibrary;
    saveThemeLibrary(themeLibrary);
    void persistThemeWorkspaceFiles(appearance, themeLibrary);

    if (appearance.presetId === themeId) {
      update({ presetId: "default" });
    } else {
      appearance = normalizeAppearance(appearance, getThemeMap(), getTypographyMap());
      saveAppearance(appearance);
      void persistTypographyWorkspaceFiles(appearance, typographyLibrary);
      apply();
    }

    return true;
  }

  function applyTheme(themeId: string): boolean {
    if (!getThemeById(themeId)) return false;
    update({ presetId: themeId });
    return true;
  }

  function isBuiltinTypography(typographyId: string): boolean {
    return !!BUILTIN_TYPOGRAPHY_PRESETS[typographyId];
  }

  function installTypography(
    typography: TypographyDefinition,
    sourceMetadata: TypographySourceMetadata = { source: "local" },
  ): boolean {
    if (!isTypographyDefinition(typography) || typography.version !== 1) {
      return false;
    }

    if (isBuiltinTypography(typography.id)) {
      // Built-ins are immutable and always available.
      return true;
    }

    const source: TypographySourceMetadata = {
      ...sourceMetadata,
      source: sourceMetadata.source ?? "local",
      installedAt: sourceMetadata.installedAt ?? Date.now(),
    };

    typographyLibrary = {
      ...typographyLibrary,
      [typography.id]: {
        typography: cloneTypography(typography),
        source,
      },
    };
    saveTypographyLibrary(typographyLibrary);
    void persistTypographyWorkspaceFiles(appearance, typographyLibrary);

    appearance = normalizeAppearance(appearance, getThemeMap(), getTypographyMap());
    saveAppearance(appearance);
    void persistThemeWorkspaceFiles(appearance, themeLibrary);
    apply();

    return true;
  }

  function uninstallTypography(typographyId: string): boolean {
    if (isBuiltinTypography(typographyId)) return false;
    if (!typographyLibrary[typographyId]) return false;

    const nextLibrary = { ...typographyLibrary };
    delete nextLibrary[typographyId];
    typographyLibrary = nextLibrary;
    saveTypographyLibrary(typographyLibrary);
    void persistTypographyWorkspaceFiles(appearance, typographyLibrary);

    if (appearance.typographyPresetId === typographyId) {
      update({
        typographyPresetId: "default",
        typographyOverrides: {},
      });
    } else {
      appearance = normalizeAppearance(appearance, getThemeMap(), getTypographyMap());
      saveAppearance(appearance);
      apply();
    }

    return true;
  }

  function applyTypographyPreset(
    typographyId: string,
    options: { preserveOverrides?: boolean } = {},
  ): boolean {
    if (!getTypographyById(typographyId)) return false;

    update({
      typographyPresetId: typographyId,
      typographyOverrides: options.preserveOverrides
        ? appearance.typographyOverrides
        : {},
    });
    return true;
  }

  function setTypographyOverride<K extends keyof TypographySettings>(
    key: K,
    value: TypographySettings[K],
  ): void {
    const preset =
      getTypographyById(appearance.typographyPresetId) ??
      BUILTIN_TYPOGRAPHY_PRESETS.default;
    const nextOverrides: Partial<TypographySettings> = {
      ...appearance.typographyOverrides,
    };

    if (preset.settings[key] === value) {
      delete nextOverrides[key];
    } else {
      nextOverrides[key] = value;
    }

    update({ typographyOverrides: nextOverrides });
  }

  // Apply on init.
  apply();

  // Re-apply when light/dark mode changes.
  if (typeof window !== "undefined") {
    const themeStore = getThemeStore();
    themeStore.onModeChange(() => apply());
  }

  async function reloadFromWorkspace(): Promise<void> {
    try {
      const [
        rawThemeSettings,
        rawThemeLibrary,
        rawTypographySettings,
        rawTypographyLibrary,
      ] = await Promise.all([
        readWorkspaceText(getThemeSettingsPath()),
        readWorkspaceText(getThemeLibraryPath()),
        readWorkspaceText(getTypographySettingsPath()),
        readWorkspaceText(getTypographyLibraryPath()),
      ]);

      const hasWorkspaceState = [
        rawThemeSettings,
        rawThemeLibrary,
        rawTypographySettings,
        rawTypographyLibrary,
      ].some((value) => typeof value === "string" && value.length > 0);

      if (!hasWorkspaceState) {
        void persistThemeWorkspaceFiles(appearance, themeLibrary);
        void persistTypographyWorkspaceFiles(appearance, typographyLibrary);
        return;
      }

      themeLibrary = rawThemeLibrary
        ? parseThemeLibraryInput(JSON.parse(rawThemeLibrary))
        : {};
      typographyLibrary = rawTypographyLibrary
        ? parseTypographyLibraryInput(JSON.parse(rawTypographyLibrary))
        : {};

      appearance = normalizeAppearance(
        {
          ...cloneDefaultAppearance(),
          ...parseThemeSettingsInput(
            rawThemeSettings ? JSON.parse(rawThemeSettings) : null,
          ),
          ...parseTypographySettingsFileInput(
            rawTypographySettings ? JSON.parse(rawTypographySettings) : null,
          ),
        },
        getThemeMap(),
        getTypographyMap(),
      );

      saveThemeLibrary(themeLibrary);
      saveTypographyLibrary(typographyLibrary);
      saveAppearance(appearance);
      apply();
    } catch (error) {
      console.warn("[appearanceStore] Failed to reload workspace appearance:", error);
    }
  }

  return {
    get appearance() {
      return appearance;
    },
    get presetId() {
      return appearance.presetId;
    },
    get typographyPresetId() {
      return appearance.typographyPresetId;
    },
    get typographyOverrides() {
      return appearance.typographyOverrides;
    },
    get accentHue() {
      return appearance.accentHue;
    },
    get typography() {
      const effective = resolveTypographySettings(appearance, getTypographyMap());
      return {
        fontFamily: effective.fontFamily,
        baseFontSize: effective.baseFontSize,
        lineHeight: effective.lineHeight,
      };
    },
    get layout() {
      const effective = resolveTypographySettings(appearance, getTypographyMap());
      return {
        contentWidth: effective.contentWidth,
      };
    },
    get activePreset(): ThemeDefinition {
      return getThemeById(appearance.presetId) ?? BUILTIN_PRESETS.default;
    },
    get activeTypographyPreset(): TypographyDefinition {
      return (
        getTypographyById(appearance.typographyPresetId) ??
        BUILTIN_TYPOGRAPHY_PRESETS.default
      );
    },

    listThemes,
    installTheme,
    uninstallTheme,
    isBuiltinTheme,
    applyTheme,

    listTypographies,
    installTypography,
    uninstallTypography,
    isBuiltinTypography,
    applyTypographyPreset,

    setPreset(themeId: string) {
      applyTheme(themeId);
    },

    setTypographyPreset(typographyId: string) {
      applyTypographyPreset(typographyId);
    },

    setAccentHue(hue: number | null) {
      update({ accentHue: hue });
    },

    setFontFamily(fontFamily: FontFamily) {
      setTypographyOverride("fontFamily", fontFamily);
    },

    setBaseFontSize(baseFontSize: number) {
      setTypographyOverride("baseFontSize", baseFontSize);
    },

    setLineHeight(lineHeight: number) {
      setTypographyOverride("lineHeight", lineHeight);
    },

    setContentWidth(contentWidth: ContentWidth) {
      setTypographyOverride("contentWidth", contentWidth);
    },

    clearTypographyOverrides() {
      update({ typographyOverrides: {} });
    },

    reloadFromWorkspace,

    reapply: apply,

    reset() {
      appearance = cloneDefaultAppearance();
      saveAppearance(appearance);
      void persistThemeWorkspaceFiles(appearance, themeLibrary);
      void persistTypographyWorkspaceFiles(appearance, typographyLibrary);
      clearCssVars();
      clearVarsCache();
    },

    exportTheme(): ThemeExport {
      const preset = getThemeById(appearance.presetId) ?? BUILTIN_PRESETS.default;
      return {
        $schema: "https://diaryx.com/schemas/theme/v1",
        theme: cloneTheme(preset),
      };
    },

    importTheme(data: unknown): boolean {
      try {
        const rawTheme = isRecord(data) && "theme" in data ? data.theme : data;
        if (!isThemeDefinition(rawTheme) || rawTheme.version !== 1) {
          return false;
        }

        const installed = installTheme(rawTheme, { source: "local" });
        if (!installed) return false;
        return applyTheme(rawTheme.id);
      } catch {
        return false;
      }
    },

    exportTypography(): TypographyExport {
      const preset =
        getTypographyById(appearance.typographyPresetId) ??
        BUILTIN_TYPOGRAPHY_PRESETS.default;
      return {
        $schema: "https://diaryx.com/schemas/typography/v1",
        typography: cloneTypography(preset),
      };
    },

    importTypography(data: unknown): boolean {
      try {
        const rawTypography =
          isRecord(data) && "typography" in data ? data.typography : data;
        if (!isTypographyDefinition(rawTypography) || rawTypography.version !== 1) {
          return false;
        }

        const installed = installTypography(rawTypography, { source: "local" });
        if (!installed) return false;
        return applyTypographyPreset(rawTypography.id);
      } catch {
        return false;
      }
    },
  };
}

let sharedStore: ReturnType<typeof createAppearanceStore> | null = null;

export function getAppearanceStore() {
  if (typeof window === "undefined") {
    return {
      get appearance() {
        return DEFAULT_APPEARANCE;
      },
      get presetId() {
        return "default";
      },
      get typographyPresetId() {
        return "default";
      },
      get typographyOverrides() {
        return {};
      },
      get accentHue() {
        return null;
      },
      get typography() {
        return {
          fontFamily: BUILTIN_TYPOGRAPHY_PRESETS.default.settings.fontFamily,
          baseFontSize: BUILTIN_TYPOGRAPHY_PRESETS.default.settings.baseFontSize,
          lineHeight: BUILTIN_TYPOGRAPHY_PRESETS.default.settings.lineHeight,
        };
      },
      get layout() {
        return {
          contentWidth: BUILTIN_TYPOGRAPHY_PRESETS.default.settings.contentWidth,
        };
      },
      get activePreset() {
        return BUILTIN_PRESETS.default;
      },
      get activeTypographyPreset() {
        return BUILTIN_TYPOGRAPHY_PRESETS.default;
      },
      listThemes: () =>
        BUILTIN_PRESET_LIST.map((theme) => ({
          theme,
          source: { source: "builtin" as const },
          builtin: true,
        })),
      installTheme: () => false,
      uninstallTheme: () => false,
      isBuiltinTheme: () => true,
      applyTheme: () => false,
      listTypographies: () =>
        BUILTIN_TYPOGRAPHY_PRESET_LIST.map((typography) => ({
          typography,
          source: { source: "builtin" as const },
          builtin: true,
        })),
      installTypography: () => false,
      uninstallTypography: () => false,
      isBuiltinTypography: () => true,
      applyTypographyPreset: () => false,
      setPreset: () => {},
      setTypographyPreset: () => {},
      setAccentHue: () => {},
      setFontFamily: () => {},
      setBaseFontSize: () => {},
      setLineHeight: () => {},
      setContentWidth: () => {},
      clearTypographyOverrides: () => {},
      reloadFromWorkspace: async () => {},
      reapply: () => {},
      reset: () => {},
      exportTheme: () => ({ $schema: "", theme: BUILTIN_PRESETS.default }),
      importTheme: () => false,
      exportTypography: () => ({
        $schema: "",
        typography: BUILTIN_TYPOGRAPHY_PRESETS.default,
      }),
      importTypography: () => false,
    } as ReturnType<typeof createAppearanceStore>;
  }

  if (!sharedStore) {
    sharedStore = createAppearanceStore();
  }

  return sharedStore;
}
