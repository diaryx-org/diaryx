/**
 * Typed API wrapper for the unified execute() command pattern.
 *
 * This module provides ergonomic, type-safe functions that wrap execute() calls.
 * Usage: `const api = createApi(backend); await api.getEntry(path);`
 */

import type { Backend, TemplateInfo } from './interface';
import {
  dispatchFileCreatedEvent,
  dispatchFileDeletedEvent,
  dispatchFileMovedEvent,
  dispatchFileSavedEvent,
} from '$lib/plugins/browserPluginManager.svelte';
import { mirrorCurrentWorkspaceMutationToLinkedProviders } from '$lib/sync/browserWorkspaceMutationMirror';
import type {
  Response,
  EntryData,
  TreeNode,
  SearchResults,
  ValidationResult,
  ValidationResultWithMeta,
  FixResult,
  FixSummary,
  ExportPlan,
  ExportedFile,
  BinaryFileInfo,
  StorageInfo,
  SearchOptions,
  AncestorAttachmentsResult,
  CreateChildResult,
  EffectiveAudienceResult,
  LinkFormat,
  WorkspaceConfig,
  PluginManifest,
} from './generated';
import type { JsonValue } from './generated/serde_json/JsonValue';

export type LinkPathType = 'workspace_root' | 'relative' | 'ambiguous';

export interface ParsedLinkResult {
  title: string | null;
  path: string;
  path_type: LinkPathType;
}

export type LinkParserOperation =
  | { type: 'parse'; params: { link: string } }
  | {
      type: 'to_canonical';
      params: {
        link: string;
        current_file_path: string;
        link_format_hint?: LinkFormat | null;
      };
    }
  | {
      type: 'format';
      params: {
        canonical_path: string;
        title: string;
        format: LinkFormat;
        from_canonical_path: string;
      };
    }
  | {
      type: 'convert';
      params: {
        link: string;
        target_format: LinkFormat;
        current_file_path: string;
        source_format_hint?: LinkFormat | null;
      };
    };

export type LinkParserResult =
  | { type: 'parsed'; data: ParsedLinkResult }
  | { type: 'string'; data: string };

// Helper to extract response data with type checking
function expectResponse<T extends Response['type']>(
  response: Response,
  expectedType: T
): Extract<Response, { type: T }> {
  if (response.type !== expectedType) {
    throw new Error(`Expected response type '${expectedType}', got '${response.type}'`);
  }
  return response as Extract<Response, { type: T }>;
}

/**
 * Create a typed API wrapper around a Backend instance.
 */
export function createApi(backend: Backend) {
  // Helper to execute a plugin command and extract the result
  async function pluginCommand(
    plugin: string,
    command: string,
    params: JsonValue = null
  ): Promise<JsonValue> {
    const response = await backend.execute({
      type: 'PluginCommand',
      params: { plugin, command, params },
    } as any);
    return expectResponse(response, 'PluginResult').data;
  }

  async function resolveAttachmentStoragePath(
    entryPath: string,
    attachmentPath: string,
  ): Promise<string> {
    const response = await backend.execute({
      type: 'ResolveAttachmentPath',
      params: { entry_path: entryPath, attachment_path: attachmentPath },
    } as any);
    return expectResponse(response, 'String').data;
  }

  async function mirrorWorkspaceMutation(): Promise<void> {
    await mirrorCurrentWorkspaceMutationToLinkedProviders({
      backend: {
        getWorkspacePath: () => backend.getWorkspacePath(),
        resolveRootIndex: async (workspacePath) => {
          const finder = (backend as { findRootIndex?: (path: string) => Promise<string> }).findRootIndex;
          return typeof finder === "function" ? await finder(workspacePath) : workspacePath;
        },
      },
      runPluginCommand: async (pluginId, command, params = null) =>
        await pluginCommand(pluginId, command, params),
    });
  }

  return {
    // =========================================================================
    // Entry Operations
    // =========================================================================

    /** Get an entry's content and metadata. */
    async getEntry(path: string): Promise<EntryData> {
      const response = await backend.execute({ type: 'GetEntry', params: { path } });
      return expectResponse(response, 'Entry').data;
    },

    /** Save an entry's content. Returns new path if H1→title sync caused a rename, null otherwise. */
    async saveEntry(path: string, content: string, rootIndexPath?: string, detectH1Title?: boolean): Promise<string | null> {
      const response = await backend.execute({ type: 'SaveEntry', params: { path, content, root_index_path: rootIndexPath ?? null, detect_h1_title: detectH1Title ?? false } } as any);
      const newPath = response.type === 'String' ? response.data : null;
      const effectivePath = newPath && newPath.length > 0 ? newPath : path;

      if (effectivePath !== path) {
        await dispatchFileMovedEvent(path, effectivePath);
        await mirrorWorkspaceMutation();
      }
      await dispatchFileSavedEvent(effectivePath);

      return newPath;
    },

    /** Create a new entry. Returns the path to the created entry. */
    async createEntry(path: string, options?: { title?: string | null; template?: string | null; part_of?: string | null; rootIndexPath?: string | null }): Promise<string> {
      const fullOptions = {
        title: options?.title ?? null,
        part_of: options?.part_of ?? null,
        template: options?.template ?? null,
        root_index_path: options?.rootIndexPath ?? null,
      };
      const response = await backend.execute({
        type: 'CreateEntry',
        params: { path, options: fullOptions },
      } as any);
      const createdPath = expectResponse(response, 'String').data;
      await dispatchFileCreatedEvent(createdPath);
      await mirrorWorkspaceMutation();
      return createdPath;
    },

    /** Delete an entry. */
    async deleteEntry(path: string): Promise<void> {
      await backend.execute({ type: 'DeleteEntry', params: { path, hard_delete: false } });
      await dispatchFileDeletedEvent(path);
      await mirrorWorkspaceMutation();
    },

    /** Move/rename an entry from one path to another. */
    async moveEntry(from: string, to: string): Promise<void> {
      await backend.execute({ type: 'MoveEntry', params: { from, to } });
      await dispatchFileMovedEvent(from, to);
      await mirrorWorkspaceMutation();
    },

    /** Rename an entry file. Returns the new path. */
    async renameEntry(path: string, newFilename: string): Promise<string> {
      const response = await backend.execute({
        type: 'RenameEntry',
        params: { path, new_filename: newFilename },
      });
      const renamedPath = expectResponse(response, 'String').data;
      await dispatchFileMovedEvent(path, renamedPath);
      await dispatchFileSavedEvent(renamedPath);
      await mirrorWorkspaceMutation();
      return renamedPath;
    },

    /** Duplicate an entry, creating a copy. Returns the new path. */
    async duplicateEntry(path: string): Promise<string> {
      const response = await backend.execute({
        type: 'DuplicateEntry',
        params: { path },
      });
      const duplicatedPath = expectResponse(response, 'String').data;
      await dispatchFileCreatedEvent(duplicatedPath);
      await mirrorWorkspaceMutation();
      return duplicatedPath;
    },

    /** Convert a leaf file to an index file with a directory. */
    async convertToIndex(path: string): Promise<string> {
      const response = await backend.execute({ type: 'ConvertToIndex', params: { path } });
      const convertedPath = expectResponse(response, 'String').data;
      await mirrorWorkspaceMutation();
      return convertedPath;
    },

    /** Convert an empty index file back to a leaf file. */
    async convertToLeaf(path: string): Promise<string> {
      const response = await backend.execute({ type: 'ConvertToLeaf', params: { path } });
      const convertedPath = expectResponse(response, 'String').data;
      await mirrorWorkspaceMutation();
      return convertedPath;
    },

    /**
     * Create a new child entry under a parent.
     * Returns detailed result including the new child path and (possibly new) parent path.
     *
     * When the parent is a leaf file, it gets converted to an index first,
     * which changes its path. The result includes both paths so the frontend
     * can correctly update the tree and navigation.
     */
    async createChildEntry(parentPath: string): Promise<CreateChildResult> {
      const response = await backend.execute({
        type: 'CreateChildEntry',
        params: { parent_path: parentPath },
      });
      const result = expectResponse(response, 'CreateChildResult').data;
      await dispatchFileCreatedEvent(result.child_path);
      if (result.parent_converted && result.original_parent_path && result.original_parent_path !== result.parent_path) {
        await dispatchFileMovedEvent(result.original_parent_path, result.parent_path);
      }
      await mirrorWorkspaceMutation();
      return result;
    },

    /** Attach an existing entry to a parent index. Returns the (possibly moved) entry path. */
    async attachEntryToParent(entryPath: string, parentPath: string): Promise<string> {
      const response = await backend.execute({
        type: 'AttachEntryToParent',
        params: { entry_path: entryPath, parent_path: parentPath },
      });
      const nextPath = expectResponse(response, 'String').data;
      if (nextPath !== entryPath) {
        await dispatchFileMovedEvent(entryPath, nextPath);
      }
      await mirrorWorkspaceMutation();
      return nextPath;
    },

    // =========================================================================
    // Workspace Operations
    // =========================================================================

    /** Find the root index file in a directory. */
    async findRootIndex(directory: string): Promise<string> {
      const response = await backend.execute({
        type: 'FindRootIndex',
        params: { directory },
      });
      return expectResponse(response, 'String').data;
    },

    /** Get the workspace tree structure. */
    async getWorkspaceTree(path?: string, depth?: number, audience?: string): Promise<TreeNode> {
      const response = await backend.execute({
        type: 'GetWorkspaceTree',
        params: { path: path ?? null, depth: depth ?? null, audience: audience ?? null },
      });
      return expectResponse(response, 'Tree').data;
    },

    /** Get the filesystem tree (for "Show All Files" mode). */
    async getFilesystemTree(path?: string, showHidden = false, depth?: number): Promise<TreeNode> {
      const response = await backend.execute({
        type: 'GetFilesystemTree',
        params: { path: path ?? null, show_hidden: showHidden, depth: depth ?? null },
      });
      return expectResponse(response, 'Tree').data;
    },

    /** Create a new workspace. */
    async createWorkspace(path?: string, name?: string): Promise<void> {
      await backend.execute({
        type: 'CreateWorkspace',
        params: { path: path ?? null, name: name ?? null },
      });
    },

    /** Get workspace configuration from root index frontmatter. */
    async getWorkspaceConfig(rootIndexPath: string): Promise<WorkspaceConfig> {
      const response = await backend.execute({
        type: 'GetWorkspaceConfig',
        params: { root_index_path: rootIndexPath },
      });
      return expectResponse(response, 'WorkspaceConfig').data;
    },

    /** Generate a filename from a title using the workspace's filename_style. Returns filename with .md extension. */
    async generateFilename(title: string, rootIndexPath?: string): Promise<string> {
      const response = await backend.execute({
        type: 'GenerateFilename',
        params: { title, root_index_path: rootIndexPath ?? null },
      } as any);
      return expectResponse(response, 'String').data;
    },

    /** Set a workspace configuration field in root index frontmatter. */
    async setWorkspaceConfig(rootIndexPath: string, field: string, value: string): Promise<void> {
      await backend.execute({
        type: 'SetWorkspaceConfig',
        params: { root_index_path: rootIndexPath, field, value },
      } as any);
      await mirrorWorkspaceMutation();
    },

    // =========================================================================
    // Frontmatter Operations
    // =========================================================================

    /** Get all frontmatter properties for an entry. */
    async getFrontmatter(path: string): Promise<Record<string, JsonValue | undefined>> {
      const response = await backend.execute({ type: 'GetFrontmatter', params: { path } });
      return expectResponse(response, 'Frontmatter').data;
    },

    /** Set a frontmatter property. Returns new path if a rename occurred (title + auto-rename), null otherwise. */
    async setFrontmatterProperty(path: string, key: string, value: JsonValue, rootIndexPath?: string): Promise<string | null> {
      const response = await backend.execute({
        type: 'SetFrontmatterProperty',
        params: { path, key, value, root_index_path: rootIndexPath ?? null },
      } as any);
      const nextPath = response.type === 'String' ? response.data : null;
      const effectivePath = nextPath && nextPath.length > 0 ? nextPath : path;
      if (effectivePath !== path) {
        await dispatchFileMovedEvent(path, effectivePath);
      }
      await dispatchFileSavedEvent(effectivePath);
      await mirrorWorkspaceMutation();
      if (response.type === 'String') {
        return response.data;
      }
      return null;
    },

    /** Remove a frontmatter property. */
    async removeFrontmatterProperty(path: string, key: string): Promise<void> {
      await backend.execute({
        type: 'RemoveFrontmatterProperty',
        params: { path, key },
      });
      await dispatchFileSavedEvent(path);
      await mirrorWorkspaceMutation();
    },

    // =========================================================================
    // Link Parser
    // =========================================================================

    /** Run a link parser operation in the Rust backend. */
    async runLinkParser(operation: LinkParserOperation): Promise<LinkParserResult> {
      const response = await backend.execute({
        type: 'LinkParser' as any,
        params: { operation } as any,
      } as any);

      if ((response as any).type !== 'LinkParserResult') {
        throw new Error(`Expected response type 'LinkParserResult', got '${response.type}'`);
      }

      return (response as any).data as LinkParserResult;
    },

    /** Parse a link string into title/path/path_type. */
    async parseLink(link: string): Promise<ParsedLinkResult> {
      const result = await this.runLinkParser({ type: 'parse', params: { link } });
      if (result.type !== 'parsed') {
        throw new Error(`Expected link parser result type 'parsed', got '${result.type}'`);
      }
      return result.data;
    },

    /** Resolve a link string to canonical workspace-relative path. */
    async canonicalizeLink(
      link: string,
      currentFilePath: string,
      linkFormatHint?: LinkFormat | null
    ): Promise<string> {
      const result = await this.runLinkParser({
        type: 'to_canonical',
        params: {
          link,
          current_file_path: currentFilePath,
          link_format_hint: linkFormatHint ?? null,
        },
      });
      if (result.type !== 'string') {
        throw new Error(`Expected link parser result type 'string', got '${result.type}'`);
      }
      return result.data;
    },

    /** Format a canonical path as a link string in the requested format. */
    async formatLink(
      canonicalPath: string,
      title: string,
      format: LinkFormat,
      fromCanonicalPath: string
    ): Promise<string> {
      const result = await this.runLinkParser({
        type: 'format',
        params: {
          canonical_path: canonicalPath,
          title,
          format,
          from_canonical_path: fromCanonicalPath,
        },
      });
      if (result.type !== 'string') {
        throw new Error(`Expected link parser result type 'string', got '${result.type}'`);
      }
      return result.data;
    },

    /** Convert an existing link string to a different format. */
    async convertLink(
      link: string,
      targetFormat: LinkFormat,
      currentFilePath: string,
      sourceFormatHint?: LinkFormat | null
    ): Promise<string> {
      const result = await this.runLinkParser({
        type: 'convert',
        params: {
          link,
          target_format: targetFormat,
          current_file_path: currentFilePath,
          source_format_hint: sourceFormatHint ?? null,
        },
      });
      if (result.type !== 'string') {
        throw new Error(`Expected link parser result type 'string', got '${result.type}'`);
      }
      return result.data;
    },

    // =========================================================================
    // Search
    // =========================================================================

    /** Search the workspace for entries. */
    async searchWorkspace(pattern: string, options?: Partial<SearchOptions>): Promise<SearchResults> {
      const fullOptions: SearchOptions = {
        workspace_path: options?.workspace_path ?? null,
        search_frontmatter: options?.search_frontmatter ?? false,
        property: options?.property ?? null,
        case_sensitive: options?.case_sensitive ?? false,
      };
      const response = await backend.execute({
        type: 'SearchWorkspace',
        params: { pattern, options: fullOptions },
      });
      return expectResponse(response, 'SearchResults').data;
    },

    // =========================================================================
    // Validation
    // =========================================================================

    /** Validate workspace links. Returns result with computed metadata. */
    async validateWorkspace(path?: string): Promise<ValidationResultWithMeta> {
      const response = await backend.execute({
        type: 'ValidateWorkspace',
        params: { path: path ?? null },
      });
      return expectResponse(response, 'ValidationResult').data;
    },

    /** Validate a single file's links. Returns result with computed metadata. */
    async validateFile(path: string): Promise<ValidationResultWithMeta> {
      const response = await backend.execute({ type: 'ValidateFile', params: { path } });
      return expectResponse(response, 'ValidationResult').data;
    },

    /** Fix a broken part_of reference. */
    async fixBrokenPartOf(path: string): Promise<FixResult> {
      const response = await backend.execute({ type: 'FixBrokenPartOf', params: { path } });
      return expectResponse(response, 'FixResult').data;
    },

    /** Fix a broken contents reference. */
    async fixBrokenContentsRef(indexPath: string, target: string): Promise<FixResult> {
      const response = await backend.execute({
        type: 'FixBrokenContentsRef',
        params: { index_path: indexPath, target },
      });
      return expectResponse(response, 'FixResult').data;
    },

    /** Fix a broken attachment reference. */
    async fixBrokenAttachment(path: string, attachment: string): Promise<FixResult> {
      const response = await backend.execute({
        type: 'FixBrokenAttachment',
        params: { path, attachment },
      });
      return expectResponse(response, 'FixResult').data;
    },

    /** Fix a non-portable path. */
    async fixNonPortablePath(
      path: string,
      property: string,
      oldValue: string,
      newValue: string
    ): Promise<FixResult> {
      const response = await backend.execute({
        type: 'FixNonPortablePath',
        params: { path, property, old_value: oldValue, new_value: newValue },
      });
      return expectResponse(response, 'FixResult').data;
    },

    /** Add an unlisted file to an index's contents. */
    async fixUnlistedFile(indexPath: string, filePath: string): Promise<FixResult> {
      const response = await backend.execute({
        type: 'FixUnlistedFile',
        params: { index_path: indexPath, file_path: filePath },
      });
      return expectResponse(response, 'FixResult').data;
    },

    /** Add an orphan binary file to an index's attachments. */
    async fixOrphanBinaryFile(indexPath: string, filePath: string): Promise<FixResult> {
      const response = await backend.execute({
        type: 'FixOrphanBinaryFile',
        params: { index_path: indexPath, file_path: filePath },
      });
      return expectResponse(response, 'FixResult').data;
    },

    /** Fix a missing part_of reference. */
    async fixMissingPartOf(filePath: string, indexPath: string): Promise<FixResult> {
      const response = await backend.execute({
        type: 'FixMissingPartOf',
        params: { file_path: filePath, index_path: indexPath },
      });
      return expectResponse(response, 'FixResult').data;
    },

    /** Fix all validation issues. */
    async fixAll(validationResult: ValidationResult): Promise<FixSummary> {
      const response = await backend.execute({
        type: 'FixAll',
        params: { validation_result: validationResult },
      });
      return expectResponse(response, 'FixSummary').data;
    },

    /** Fix a circular reference by removing a contents reference. */
    async fixCircularReference(filePath: string, contentsRefToRemove: string): Promise<FixResult> {
      const response = await backend.execute({
        type: 'FixCircularReference',
        params: { file_path: filePath, part_of_value: contentsRefToRemove },
      });
      return expectResponse(response, 'FixResult').data;
    },

    /** Get available parent indexes for a file (for "Choose parent" picker). */
    async getAvailableParentIndexes(filePath: string, workspaceRoot: string): Promise<string[]> {
      const response = await backend.execute({
        type: 'GetAvailableParentIndexes',
        params: { file_path: filePath, workspace_root: workspaceRoot },
      });
      return expectResponse(response, 'Strings').data;
    },

    // =========================================================================
    // Export
    // =========================================================================

    /** Get the effective audience for an entry, resolving inheritance. */
    async getEffectiveAudience(path: string): Promise<EffectiveAudienceResult> {
      const response = await backend.execute({
        type: 'GetEffectiveAudience',
        params: { path },
      } as any);
      return (response as any).data as EffectiveAudienceResult;
    },

    /** Get available audiences. */
    async getAvailableAudiences(rootPath: string): Promise<string[]> {
      const response = await backend.execute({
        type: 'GetAvailableAudiences',
        params: { path: rootPath },
      });
      return expectResponse(response, 'Strings').data;
    },

    /** Plan an export operation. */
    async planExport(rootPath: string, audience: string): Promise<ExportPlan> {
      const result = await pluginCommand('publish', 'PlanExport', {
        root_path: rootPath,
        audience,
      });
      return result as ExportPlan;
    },

    /** Export to memory. */
    async exportToMemory(rootPath: string, audience: string): Promise<ExportedFile[]> {
      const result = await pluginCommand('publish', 'ExportToMemory', {
        root_path: rootPath,
        audience,
      });
      return (result ?? []) as ExportedFile[];
    },

    /** Export to HTML. */
    async exportToHtml(rootPath: string, audience: string): Promise<ExportedFile[]> {
      const result = await pluginCommand('publish', 'ExportToHtml', {
        root_path: rootPath,
        audience,
      });
      return (result ?? []) as ExportedFile[];
    },

    /** Export binary attachments (returns paths only, use readBinary to get data). */
    async exportBinaryAttachments(rootPath: string, audience: string): Promise<BinaryFileInfo[]> {
      const result = await pluginCommand('publish', 'ExportBinaryAttachments', {
        root_path: rootPath,
        audience,
      });
      return (result ?? []) as BinaryFileInfo[];
    },

    // =========================================================================
    // Templates (via diaryx.templating plugin)
    // =========================================================================

    /** List available templates. */
    async listTemplates(workspacePath?: string): Promise<TemplateInfo[]> {
      const result = await pluginCommand('diaryx.templating', 'ListTemplates', {
        workspace_path: workspacePath ?? null,
      });
      return (result ?? []) as unknown as TemplateInfo[];
    },

    /** Get a template's content. */
    async getTemplate(name: string, workspacePath?: string): Promise<string> {
      const result = await pluginCommand('diaryx.templating', 'GetTemplate', {
        name,
        workspace_path: workspacePath ?? null,
      });
      return result as string;
    },

    /** Save a template. */
    async saveTemplate(name: string, content: string, workspacePath: string): Promise<void> {
      await pluginCommand('diaryx.templating', 'SaveTemplate', {
        name,
        content,
        workspace_path: workspacePath,
      });
    },

    /** Delete a template. */
    async deleteTemplate(name: string, workspacePath: string): Promise<void> {
      await pluginCommand('diaryx.templating', 'DeleteTemplate', {
        name,
        workspace_path: workspacePath,
      });
    },

    // =========================================================================
    // Attachments
    // =========================================================================

    /** Get attachments for an entry. */
    async getAttachments(path: string): Promise<string[]> {
      const response = await backend.execute({ type: 'GetAttachments', params: { path } });
      return expectResponse(response, 'Strings').data;
    },

    /** Upload an attachment over the binary backend path and register it in frontmatter. */
    async uploadAttachment(entryPath: string, filename: string, data: Uint8Array): Promise<string> {
      const attachmentPath = `_attachments/${filename}`;
      const storagePath = await resolveAttachmentStoragePath(entryPath, attachmentPath);
      await backend.writeBinary(storagePath, data);

      const response = await backend.execute({
        type: 'RegisterAttachment',
        params: { entry_path: entryPath, filename },
      });
      return expectResponse(response, 'String').data;
    },

    /** Delete an attachment. */
    async deleteAttachment(entryPath: string, attachmentPath: string): Promise<void> {
      await backend.execute({
        type: 'DeleteAttachment',
        params: { entry_path: entryPath, attachment_path: attachmentPath },
      });
    },

    /** Get attachment data. */
    async getAttachmentData(entryPath: string, attachmentPath: string): Promise<number[]> {
      const response = await backend.execute({
        type: 'GetAttachmentData',
        params: { entry_path: entryPath, attachment_path: attachmentPath },
      });
      return expectResponse(response, 'Bytes').data;
    },

    /** Resolve an attachment path to its storage path (for use with readBinary). */
    async resolveAttachmentStoragePath(entryPath: string, attachmentPath: string): Promise<string> {
      return resolveAttachmentStoragePath(entryPath, attachmentPath);
    },

    /** Move an attachment from one entry to another. Returns the new attachment path. */
    async moveAttachment(
      sourceEntryPath: string,
      targetEntryPath: string,
      attachmentPath: string,
      newFilename?: string
    ): Promise<string> {
      const response = await backend.execute({
        type: 'MoveAttachment',
        params: {
          source_entry_path: sourceEntryPath,
          target_entry_path: targetEntryPath,
          attachment_path: attachmentPath,
          new_filename: newFilename ?? null,
        },
      });
      return expectResponse(response, 'String').data;
    },

    /** Get attachments from current entry and all ancestor indexes in the part_of chain. */
    async getAncestorAttachments(path: string): Promise<AncestorAttachmentsResult> {
      const response = await backend.execute({
        type: 'GetAncestorAttachments',
        params: { path },
      });
      return expectResponse(response, 'AncestorAttachments').data;
    },

    // =========================================================================
    // File System
    // =========================================================================

    /** Check if a file exists. */
    async fileExists(path: string): Promise<boolean> {
      const response = await backend.execute({ type: 'FileExists', params: { path } });
      return expectResponse(response, 'Bool').data;
    },

    /** Read a file's content. */
    async readFile(path: string): Promise<string> {
      const response = await backend.execute({ type: 'ReadFile', params: { path } });
      return expectResponse(response, 'String').data;
    },

    /** Write content to a file. */
    async writeFile(path: string, content: string): Promise<void> {
      await backend.execute({ type: 'WriteFile', params: { path, content } });
    },

    /** Delete a file. */
    async deleteFile(path: string): Promise<void> {
      await backend.execute({ type: 'DeleteFile', params: { path } });
    },

    /**
     * Write a file with metadata as YAML frontmatter + body content.
     * Generates the YAML frontmatter from the metadata and writes it to the file.
     */
    async writeFileWithMetadata(path: string, metadata: JsonValue, body: string): Promise<void> {
      await backend.execute({
        type: 'WriteFileWithMetadata',
        params: { path, metadata, body },
      });
    },

    /**
     * Update file's frontmatter metadata, preserving or replacing the body.
     * If body is provided, it replaces the existing body.
     */
    async updateFileMetadata(path: string, metadata: JsonValue, body?: string): Promise<void> {
      await backend.execute({
        type: 'UpdateFileMetadata',
        params: { path, metadata, body: body ?? null },
      });
    },

    /** Read a binary file's content. */
    async readBinary(path: string): Promise<Uint8Array> {
      return backend.readBinary(path);
    },

    /** Write binary content to a file. */
    async writeBinary(path: string, data: Uint8Array): Promise<void> {
      return backend.writeBinary(path, data);
    },

    // =========================================================================
    // Naming / URL Validation
    // =========================================================================

    /**
     * Validate and normalize a workspace name for creation.
     * Checks non-empty and uniqueness against local/server names.
     * Returns the trimmed name on success, throws on validation failure.
     */
    async validateWorkspaceName(
      name: string,
      existingLocalNames: string[],
      existingServerNames?: string[],
    ): Promise<string> {
      const response = await backend.execute({
        type: 'ValidateWorkspaceName',
        params: {
          name,
          existing_local_names: existingLocalNames,
          existing_server_names: existingServerNames ?? null,
        },
      } as any);
      return expectResponse(response, 'String').data;
    },

    /**
     * Validate a publishing site slug.
     * Must be 3–64 lowercase letters, digits, or hyphens.
     * Throws on validation failure.
     */
    async validatePublishingSlug(slug: string): Promise<void> {
      await backend.execute({
        type: 'ValidatePublishingSlug',
        params: { slug },
      } as any);
    },

    /**
     * Normalize a server URL: trim whitespace, add https:// if no scheme.
     */
    async normalizeServerUrl(url: string): Promise<string> {
      const response = await backend.execute({
        type: 'NormalizeServerUrl',
        params: { url },
      } as any);
      return expectResponse(response, 'String').data;
    },

    // =========================================================================
    // Storage
    // =========================================================================

    /** Get storage usage information. */
    async getStorageUsage(): Promise<StorageInfo> {
      const response = await backend.execute({ type: 'GetStorageUsage' });
      return expectResponse(response, 'StorageInfo').data;
    },

    // =========================================================================
    // Plugin Operations
    // =========================================================================

    /** Get manifests for all registered plugins. */
    async getPluginManifests(): Promise<PluginManifest[]> {
      const response = await backend.execute({
        type: 'GetPluginManifests',
      } as any);
      return expectResponse(response, 'PluginManifests').data;
    },

    /** Get a plugin's configuration. */
    async getPluginConfig(plugin: string): Promise<JsonValue> {
      const response = await backend.execute({
        type: 'GetPluginConfig',
        params: { plugin },
      });
      return expectResponse(response, 'PluginResult').data;
    },

    /** Set a plugin's configuration. */
    async setPluginConfig(plugin: string, config: JsonValue): Promise<void> {
      await backend.execute({
        type: 'SetPluginConfig',
        params: { plugin, config },
      });
    },

    /** Execute a plugin-specific command. */
    executePluginCommand: pluginCommand,
  };
}

/** Type of the API wrapper returned by createApi(). */
export type Api = ReturnType<typeof createApi>;
