import type { RegistryPlugin } from "$lib/plugins/pluginRegistry";
import { installRegistryPlugin } from "$lib/plugins/pluginInstallService";
import { getAppearanceStore } from "$lib/stores/appearance.svelte";
import { getPluginStore } from "@/models/stores/pluginStore.svelte";

import type {
  BundleRegistryEntry,
  BundleTypographyPreset,
  ThemeRegistryEntry,
  TypographyRegistryEntry,
} from "./types";

export interface BundleApplyRuntime {
  hasTheme(themeId: string): boolean;
  installTheme(theme: ThemeRegistryEntry): void;
  applyTheme(themeId: string): void;
  hasTypographyPreset(typographyId: string): boolean;
  installTypography(typography: TypographyRegistryEntry): void;
  applyTypographyPreset(typographyId: string): void;
  applyTypographyOverrides(preset: BundleTypographyPreset): void;
  isPluginInstalled(pluginId: string): boolean;
  installRegistryPlugin(plugin: RegistryPlugin): Promise<void>;
  setPluginEnabled(pluginId: string, enabled: boolean): void;
}

export interface BundlePlanContext {
  themes: ThemeRegistryEntry[];
  typographies: TypographyRegistryEntry[];
  plugins: RegistryPlugin[];
  runtime?: BundleApplyRuntime;
}

export type BundleActionType =
  | "theme-install"
  | "theme-apply"
  | "typography-install"
  | "typography-preset-apply"
  | "typography-override-apply"
  | "plugin-install"
  | "plugin-enable";

export interface BundlePlanAction {
  type: BundleActionType;
  key: string;
  required: boolean;
  label: string;
  theme?: ThemeRegistryEntry;
  themeId?: string;
  typographyEntry?: TypographyRegistryEntry;
  typographyId?: string;
  typography?: BundleTypographyPreset;
  plugin?: RegistryPlugin;
  pluginId?: string;
  pluginEnable?: boolean;
}

export interface BundleApplyPlan {
  bundle: BundleRegistryEntry;
  actions: BundlePlanAction[];
  missingRequiredPlugins: string[];
  missingOptionalPlugins: string[];
  missingTheme: boolean;
  missingTypographyPreset: boolean;
}

export type BundleApplyResultStatus = "success" | "failed";

export interface BundleApplyActionResult {
  action: BundlePlanAction;
  status: BundleApplyResultStatus;
  message: string;
}

export interface BundleApplyResult {
  plan: BundleApplyPlan;
  results: BundleApplyActionResult[];
  summary: {
    total: number;
    success: number;
    failed: number;
  };
}

export function createDefaultBundleApplyRuntime(): BundleApplyRuntime {
  const appearanceStore = getAppearanceStore();
  const pluginStore = getPluginStore();

  return {
    hasTheme(themeId: string): boolean {
      return appearanceStore.listThemes().some((entry) => entry.theme.id === themeId);
    },
    installTheme(theme: ThemeRegistryEntry): void {
      appearanceStore.installTheme(theme.theme, {
        source: "registry",
        registryId: theme.id,
      });
    },
    applyTheme(themeId: string): void {
      const ok = appearanceStore.applyTheme(themeId);
      if (!ok) {
        throw new Error(`Theme '${themeId}' is not available`);
      }
    },
    hasTypographyPreset(typographyId: string): boolean {
      return appearanceStore
        .listTypographies()
        .some((entry) => entry.typography.id === typographyId);
    },
    installTypography(typography: TypographyRegistryEntry): void {
      appearanceStore.installTypography(typography.typography, {
        source: "registry",
        registryId: typography.id,
      });
    },
    applyTypographyPreset(typographyId: string): void {
      const ok = appearanceStore.applyTypographyPreset(typographyId);
      if (!ok) {
        throw new Error(`Typography preset '${typographyId}' is not available`);
      }
    },
    applyTypographyOverrides(preset: BundleTypographyPreset): void {
      if (preset.fontFamily) {
        appearanceStore.setFontFamily(preset.fontFamily);
      }
      if (typeof preset.baseFontSize === "number") {
        appearanceStore.setBaseFontSize(preset.baseFontSize);
      }
      if (typeof preset.lineHeight === "number") {
        appearanceStore.setLineHeight(preset.lineHeight);
      }
      if (preset.contentWidth) {
        appearanceStore.setContentWidth(preset.contentWidth);
      }
    },
    isPluginInstalled(pluginId: string): boolean {
      return pluginStore.allManifests.some((manifest) => String(manifest.id) === pluginId);
    },
    installRegistryPlugin,
    setPluginEnabled(pluginId: string, enabled: boolean): void {
      pluginStore.setPluginEnabled(pluginId, enabled);
    },
  };
}

export function planBundleApply(
  bundle: BundleRegistryEntry,
  context: BundlePlanContext,
): BundleApplyPlan {
  const runtime = context.runtime ?? createDefaultBundleApplyRuntime();
  const themeById = new Map(context.themes.map((theme) => [theme.id, theme]));
  const typographyById = new Map(
    context.typographies.map((typography) => [typography.id, typography]),
  );
  const pluginById = new Map(context.plugins.map((plugin) => [plugin.id, plugin]));

  const actions: BundlePlanAction[] = [];
  const missingRequiredPlugins: string[] = [];
  const missingOptionalPlugins: string[] = [];

  const theme = themeById.get(bundle.theme_id);
  const missingTheme = !theme;
  const themeInstalled = runtime.hasTheme(bundle.theme_id);

  if (theme && !themeInstalled) {
    actions.push({
      type: "theme-install",
      key: `theme-install:${theme.id}`,
      required: true,
      label: `Install theme ${theme.name}`,
      theme,
      themeId: theme.id,
    });
  }

  actions.push({
    type: "theme-apply",
    key: `theme-apply:${bundle.theme_id}`,
    required: true,
    label: `Apply theme ${bundle.theme_id}`,
    themeId: bundle.theme_id,
    theme,
  });

  const typographyId = bundle.typography_id;
  const typographyEntry = typographyId ? typographyById.get(typographyId) : undefined;
  const missingTypographyPreset = !!typographyId && !typographyEntry;

  if (typographyId && typographyEntry && !runtime.hasTypographyPreset(typographyId)) {
    actions.push({
      type: "typography-install",
      key: `typography-install:${typographyId}`,
      required: false,
      label: `Install typography ${typographyEntry.name}`,
      typographyId,
      typographyEntry,
    });
  }

  if (typographyId) {
    actions.push({
      type: "typography-preset-apply",
      key: `typography-preset-apply:${typographyId}`,
      required: false,
      label: `Apply typography ${typographyId}`,
      typographyId,
      typographyEntry,
    });
  }

  if (bundle.typography) {
    actions.push({
      type: "typography-override-apply",
      key: `typography-overrides:${bundle.id}`,
      required: false,
      label: "Apply typography overrides",
      typography: bundle.typography,
    });
  }

  for (const dependency of bundle.plugins) {
    const required = dependency.required ?? true;
    const shouldEnable = dependency.enable ?? true;
    const plugin = pluginById.get(dependency.plugin_id);
    const installed = runtime.isPluginInstalled(dependency.plugin_id);

    if (!installed) {
      if (!plugin) {
        if (required) {
          missingRequiredPlugins.push(dependency.plugin_id);
        } else {
          missingOptionalPlugins.push(dependency.plugin_id);
        }
      } else {
        actions.push({
          type: "plugin-install",
          key: `plugin-install:${dependency.plugin_id}`,
          required,
          label: `Install plugin ${dependency.plugin_id}`,
          plugin,
          pluginId: dependency.plugin_id,
        });
      }
    }

    if (shouldEnable) {
      actions.push({
        type: "plugin-enable",
        key: `plugin-enable:${dependency.plugin_id}`,
        required,
        label: `Enable plugin ${dependency.plugin_id}`,
        pluginId: dependency.plugin_id,
        pluginEnable: true,
      });
    }
  }

  return {
    bundle,
    actions,
    missingRequiredPlugins,
    missingOptionalPlugins,
    missingTheme,
    missingTypographyPreset,
  };
}

export async function executeBundleApply(
  plan: BundleApplyPlan,
  runtime: BundleApplyRuntime = createDefaultBundleApplyRuntime(),
): Promise<BundleApplyResult> {
  const results: BundleApplyActionResult[] = [];

  for (const action of plan.actions) {
    try {
      if (action.type === "theme-install") {
        if (!action.theme) {
          throw new Error("Theme metadata is unavailable");
        }
        runtime.installTheme(action.theme);
      } else if (action.type === "theme-apply") {
        if (!action.themeId) {
          throw new Error("Theme ID is unavailable");
        }
        runtime.applyTheme(action.themeId);
      } else if (action.type === "typography-install") {
        if (!action.typographyEntry) {
          throw new Error("Typography metadata is unavailable");
        }
        runtime.installTypography(action.typographyEntry);
      } else if (action.type === "typography-preset-apply") {
        if (!action.typographyId) {
          throw new Error("Typography preset ID is unavailable");
        }
        runtime.applyTypographyPreset(action.typographyId);
      } else if (action.type === "typography-override-apply") {
        if (!action.typography) {
          throw new Error("Typography overrides are unavailable");
        }
        runtime.applyTypographyOverrides(action.typography);
      } else if (action.type === "plugin-install") {
        if (!action.plugin) {
          throw new Error("Plugin metadata is unavailable");
        }
        await runtime.installRegistryPlugin(action.plugin);
      } else if (action.type === "plugin-enable") {
        if (!action.pluginId) {
          throw new Error("Plugin ID is unavailable");
        }
        const installed = runtime.isPluginInstalled(action.pluginId);
        if (!installed) {
          throw new Error("Plugin is not installed");
        }
        runtime.setPluginEnabled(action.pluginId, true);
      }

      results.push({
        action,
        status: "success",
        message: "Completed",
      });
    } catch (error) {
      results.push({
        action,
        status: "failed",
        message: error instanceof Error ? error.message : String(error),
      });
    }
  }

  if (plan.missingTheme) {
    results.push({
      action: {
        type: "theme-apply",
        key: `theme-missing:${plan.bundle.theme_id}`,
        required: true,
        label: `Missing theme ${plan.bundle.theme_id}`,
        themeId: plan.bundle.theme_id,
      },
      status: "failed",
      message: `Theme '${plan.bundle.theme_id}' was not found in the theme registry`,
    });
  }

  if (plan.missingTypographyPreset && plan.bundle.typography_id) {
    results.push({
      action: {
        type: "typography-preset-apply",
        key: `typography-missing:${plan.bundle.typography_id}`,
        required: false,
        label: `Missing typography ${plan.bundle.typography_id}`,
        typographyId: plan.bundle.typography_id,
      },
      status: "failed",
      message: `Typography preset '${plan.bundle.typography_id}' was not found in the typography registry`,
    });
  }

  for (const pluginId of plan.missingRequiredPlugins) {
    results.push({
      action: {
        type: "plugin-install",
        key: `plugin-missing-required:${pluginId}`,
        required: true,
        label: `Missing required plugin ${pluginId}`,
        pluginId,
      },
      status: "failed",
      message: `Required plugin '${pluginId}' was not found in the plugin registry`,
    });
  }

  for (const pluginId of plan.missingOptionalPlugins) {
    results.push({
      action: {
        type: "plugin-install",
        key: `plugin-missing-optional:${pluginId}`,
        required: false,
        label: `Missing optional plugin ${pluginId}`,
        pluginId,
      },
      status: "failed",
      message: `Optional plugin '${pluginId}' was not found in the plugin registry`,
    });
  }

  const success = results.filter((result) => result.status === "success").length;
  const failed = results.length - success;

  return {
    plan,
    results,
    summary: {
      total: results.length,
      success,
      failed,
    },
  };
}
