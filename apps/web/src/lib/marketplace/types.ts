import type {
  ThemeDefinition,
  TypographyDefinition,
  TypographySettings,
} from "$lib/stores/appearance.types";

export type MarketplaceKind = "plugin" | "theme" | "typography" | "bundle" | "template" | "starter-workspace";

export interface MarketplaceArtifact {
  url: string;
  sha256: string;
  size: number;
  published_at: string;
}

export interface ThemeRegistryEntry {
  kind: "theme";
  id: string;
  name: string;
  version: string;
  summary: string;
  description: string;
  author: string;
  license: string;
  repository: string | null;
  categories: string[];
  tags: string[];
  styles: string[];
  icon: string | null;
  screenshots: string[];
  artifact: MarketplaceArtifact | null;
  theme: ThemeDefinition;
}

export interface TypographyRegistryEntry {
  kind: "typography";
  id: string;
  name: string;
  version: string;
  summary: string;
  description: string;
  author: string;
  license: string;
  repository: string | null;
  categories: string[];
  tags: string[];
  styles: string[];
  icon: string | null;
  screenshots: string[];
  artifact: MarketplaceArtifact | null;
  typography: TypographyDefinition;
}

export interface BundlePluginDependency {
  plugin_id: string;
  required: boolean;
  enable: boolean;
}

export type BundleTypographyPreset = Partial<TypographySettings>;

export interface BundleRegistryEntry {
  kind: "bundle";
  id: string;
  name: string;
  version: string;
  summary: string;
  description: string;
  author: string;
  license: string;
  repository: string | null;
  categories: string[];
  tags: string[];
  icon: string | null;
  screenshots: string[];
  artifact: MarketplaceArtifact | null;
  theme_id: string;
  typography_id: string | null;
  typography: BundleTypographyPreset | null;
  plugins: BundlePluginDependency[];
}

export interface TemplateRegistryEntry {
  kind: "template";
  id: string;
  name: string;
  version: string;
  summary: string;
  description: string;
  author: string;
  license: string;
  repository: string | null;
  categories: string[];
  tags: string[];
  icon: string | null;
  screenshots: string[];
  artifact: MarketplaceArtifact | null;
  template_variables: string[];
  preview: string | null;
}

export interface StarterWorkspaceFile {
  path: string;
  content: string;
}

export interface StarterWorkspaceRegistryEntry {
  kind: "starter-workspace";
  id: string;
  name: string;
  version: string;
  summary: string;
  description: string;
  author: string;
  license: string;
  repository: string | null;
  categories: string[];
  tags: string[];
  icon: string | null;
  screenshots: string[];
  artifact: MarketplaceArtifact | null;
  file_count: number;
  bundle_id: string | null;
  includes_templates: boolean;
}
