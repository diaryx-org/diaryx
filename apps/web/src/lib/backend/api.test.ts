import { describe, it, expect, vi, beforeEach } from 'vitest'

const browserPluginEventMocks = vi.hoisted(() => ({
  dispatchFileCreatedEvent: vi.fn().mockResolvedValue(undefined),
  dispatchFileDeletedEvent: vi.fn().mockResolvedValue(undefined),
  dispatchFileMovedEvent: vi.fn().mockResolvedValue(undefined),
  dispatchFileSavedEvent: vi.fn().mockResolvedValue(undefined),
}))

const workspaceMirrorMocks = vi.hoisted(() => ({
  mirrorCurrentWorkspaceMutationToLinkedProviders: vi.fn().mockResolvedValue(undefined),
}))

vi.mock('$lib/plugins/browserPluginManager.svelte', () => browserPluginEventMocks)
vi.mock('$lib/sync/browserWorkspaceMutationMirror', () => workspaceMirrorMocks)

import { createApi } from './api'
import type { Backend } from './interface'

describe('api', () => {
  let mockBackend: Backend
  let api: ReturnType<typeof createApi>

  beforeEach(() => {
    mockBackend = {
      init: vi.fn().mockResolvedValue(undefined),
      isReady: vi.fn().mockReturnValue(true),
      getWorkspacePath: vi.fn().mockReturnValue('workspace/index.md'),
      getConfig: vi.fn().mockReturnValue(null),
      getAppPaths: vi.fn().mockReturnValue(null),
      execute: vi.fn(),
      on: vi.fn(),
      off: vi.fn(),
      persist: vi.fn().mockResolvedValue(undefined),
      readBinary: vi.fn().mockResolvedValue(new Uint8Array()),
      writeBinary: vi.fn().mockResolvedValue(undefined),
      importFromZip: vi.fn().mockResolvedValue({ success: true, files_imported: 0 }),
    }
    api = createApi(mockBackend)
    vi.clearAllMocks()
  })

  describe('getEntry', () => {
    it('should get entry by path', async () => {
      const mockEntry = {
        path: 'test.md',
        title: 'Test',
        content: '# Test',
        frontmatter: { title: 'Test' },
      }
      vi.mocked(mockBackend.execute).mockResolvedValue({
        type: 'Entry',
        data: mockEntry,
      })

      const result = await api.getEntry('test.md')

      expect(mockBackend.execute).toHaveBeenCalledWith({
        type: 'GetEntry',
        params: { path: 'test.md' },
      })
      expect(result).toEqual(mockEntry)
    })

    it('should throw on unexpected response type', async () => {
      vi.mocked(mockBackend.execute).mockResolvedValue({
        type: 'Ok',
      })

      await expect(api.getEntry('test.md')).rejects.toThrow(
        "Expected response type 'Entry', got 'Ok'"
      )
    })
  })

  describe('saveEntry', () => {
    it('should save entry content', async () => {
      vi.mocked(mockBackend.execute).mockResolvedValue({ type: 'Ok' })

      await api.saveEntry('test.md', '# Updated Content')

      expect(mockBackend.execute).toHaveBeenCalledWith({
        type: 'SaveEntry',
        params: { path: 'test.md', content: '# Updated Content', root_index_path: null, detect_h1_title: false },
      })
      expect(browserPluginEventMocks.dispatchFileSavedEvent).toHaveBeenCalledWith('test.md', { bodyChanged: true })
      expect(browserPluginEventMocks.dispatchFileMovedEvent).not.toHaveBeenCalled()
      expect(workspaceMirrorMocks.mirrorCurrentWorkspaceMutationToLinkedProviders).toHaveBeenCalledTimes(1)
    })

    it('should mirror workspace metadata after a save-driven rename', async () => {
      vi.mocked(mockBackend.execute).mockResolvedValue({
        type: 'String',
        data: 'renamed.md',
      })

      const result = await api.saveEntry('test.md', '# Updated Content', 'README.md', true)

      expect(result).toBe('renamed.md')
      expect(browserPluginEventMocks.dispatchFileMovedEvent).toHaveBeenCalledWith('test.md', 'renamed.md')
      expect(browserPluginEventMocks.dispatchFileSavedEvent).toHaveBeenCalledWith('renamed.md', { bodyChanged: true })
      expect(workspaceMirrorMocks.mirrorCurrentWorkspaceMutationToLinkedProviders).toHaveBeenCalledTimes(1)
    })
  })

  describe('createEntry', () => {
    it('should create entry with default options', async () => {
      vi.mocked(mockBackend.execute).mockResolvedValue({
        type: 'String',
        data: 'new-entry.md',
      })

      const result = await api.createEntry('new-entry.md')

      expect(mockBackend.execute).toHaveBeenCalledWith({
        type: 'CreateEntry',
        params: {
          path: 'new-entry.md',
          options: { title: null, part_of: null, template: null, root_index_path: null },
        },
      })
      expect(result).toBe('new-entry.md')
      expect(browserPluginEventMocks.dispatchFileCreatedEvent).toHaveBeenCalledWith('new-entry.md')
      expect(workspaceMirrorMocks.mirrorCurrentWorkspaceMutationToLinkedProviders).toHaveBeenCalledTimes(1)
    })

    it('should create entry with options', async () => {
      vi.mocked(mockBackend.execute).mockResolvedValue({
        type: 'String',
        data: 'new-entry.md',
      })

      await api.createEntry('new-entry.md', {
        title: 'New Entry',
        template: 'note',
        part_of: 'index.md',
      })

      expect(mockBackend.execute).toHaveBeenCalledWith({
        type: 'CreateEntry',
        params: {
          path: 'new-entry.md',
          options: { title: 'New Entry', part_of: 'index.md', template: 'note', root_index_path: null },
        },
      })
      expect(browserPluginEventMocks.dispatchFileCreatedEvent).toHaveBeenCalledWith('new-entry.md')
      expect(workspaceMirrorMocks.mirrorCurrentWorkspaceMutationToLinkedProviders).toHaveBeenCalledTimes(1)
    })
  })

  describe('deleteEntry', () => {
    it('should delete entry', async () => {
      vi.mocked(mockBackend.execute).mockResolvedValue({ type: 'Ok' })

      await api.deleteEntry('test.md')

      expect(mockBackend.execute).toHaveBeenCalledWith({
        type: 'DeleteEntry',
        params: { path: 'test.md', hard_delete: false },
      })
      expect(browserPluginEventMocks.dispatchFileDeletedEvent).toHaveBeenCalledWith('test.md')
      expect(workspaceMirrorMocks.mirrorCurrentWorkspaceMutationToLinkedProviders).toHaveBeenCalledTimes(1)
    })
  })

  describe('moveEntry', () => {
    it('should move entry', async () => {
      vi.mocked(mockBackend.execute).mockResolvedValue({ type: 'Ok' })

      await api.moveEntry('old/path.md', 'new/path.md')

      expect(mockBackend.execute).toHaveBeenCalledWith({
        type: 'MoveEntry',
        params: { from: 'old/path.md', to: 'new/path.md' },
      })
      expect(browserPluginEventMocks.dispatchFileMovedEvent).toHaveBeenCalledWith('old/path.md', 'new/path.md')
      expect(workspaceMirrorMocks.mirrorCurrentWorkspaceMutationToLinkedProviders).toHaveBeenCalledTimes(1)
    })
  })

  describe('renameEntry', () => {
    it('should rename entry', async () => {
      vi.mocked(mockBackend.execute).mockResolvedValue({
        type: 'String',
        data: 'folder/new-name.md',
      })

      const result = await api.renameEntry('folder/old-name.md', 'new-name.md')

      expect(mockBackend.execute).toHaveBeenCalledWith({
        type: 'RenameEntry',
        params: { path: 'folder/old-name.md', new_filename: 'new-name.md' },
      })
      expect(result).toBe('folder/new-name.md')
      expect(browserPluginEventMocks.dispatchFileMovedEvent).toHaveBeenCalledWith('folder/old-name.md', 'folder/new-name.md')
      expect(browserPluginEventMocks.dispatchFileSavedEvent).toHaveBeenCalledWith('folder/new-name.md', { bodyChanged: false })
      expect(workspaceMirrorMocks.mirrorCurrentWorkspaceMutationToLinkedProviders).toHaveBeenCalledTimes(1)
    })
  })

  describe('attachEntryToParent', () => {
    it('should emit a move event when the entry path changes', async () => {
      vi.mocked(mockBackend.execute).mockResolvedValue({
        type: 'String',
        data: 'folder/child.md',
      })

      const result = await api.attachEntryToParent('child.md', 'folder/index.md')

      expect(mockBackend.execute).toHaveBeenCalledWith({
        type: 'AttachEntryToParent',
        params: { entry_path: 'child.md', parent_path: 'folder/index.md' },
      })
      expect(result).toBe('folder/child.md')
      expect(browserPluginEventMocks.dispatchFileMovedEvent).toHaveBeenCalledWith('child.md', 'folder/child.md')
      expect(browserPluginEventMocks.dispatchFileSavedEvent).not.toHaveBeenCalled()
      expect(workspaceMirrorMocks.mirrorCurrentWorkspaceMutationToLinkedProviders).toHaveBeenCalledTimes(1)
    })

    it('should emit a save event when only relationship metadata changes', async () => {
      vi.mocked(mockBackend.execute).mockResolvedValue({
        type: 'String',
        data: 'child.md',
      })

      const result = await api.attachEntryToParent('child.md', 'folder/index.md')

      expect(result).toBe('child.md')
      expect(browserPluginEventMocks.dispatchFileMovedEvent).not.toHaveBeenCalled()
      expect(browserPluginEventMocks.dispatchFileSavedEvent).toHaveBeenCalledWith('child.md', { bodyChanged: false })
      expect(workspaceMirrorMocks.mirrorCurrentWorkspaceMutationToLinkedProviders).toHaveBeenCalledTimes(1)
    })
  })

  describe('getWorkspaceTree', () => {
    it('should get workspace tree', async () => {
      const mockTree = {
        path: 'workspace',
        name: 'workspace',
        description: null,
        children: [],
        properties: {},
      }
      vi.mocked(mockBackend.execute).mockResolvedValue({
        type: 'Tree',
        data: mockTree,
      })

      const result = await api.getWorkspaceTree()

      expect(mockBackend.execute).toHaveBeenCalledWith({
        type: 'GetWorkspaceTree',
        params: { path: null, depth: null, audience: null },
      })
      expect(result).toEqual(mockTree)
    })

    it('should get workspace tree with path and depth', async () => {
      vi.mocked(mockBackend.execute).mockResolvedValue({
        type: 'Tree',
        data: { path: 'subdir', name: 'subdir', description: null, children: [], properties: {} },
      })

      await api.getWorkspaceTree('subdir', 2)

      expect(mockBackend.execute).toHaveBeenCalledWith({
        type: 'GetWorkspaceTree',
        params: { path: 'subdir', depth: 2, audience: null },
      })
    })
  })

  describe('validateWorkspace', () => {
    it('should validate workspace', async () => {
      const mockResult = {
        errors: [],
        warnings: [],
        files_checked: 10,
      }
      vi.mocked(mockBackend.execute).mockResolvedValue({
        type: 'ValidationResult',
        data: mockResult,
      })

      const result = await api.validateWorkspace()

      expect(mockBackend.execute).toHaveBeenCalledWith({
        type: 'ValidateWorkspace',
        params: { path: null },
      })
      expect(result).toEqual(mockResult)
    })
  })

  describe('searchWorkspace', () => {
    it('should search workspace with default options', async () => {
      const mockResults = {
        files: [],
        files_searched: 10,
      }
      vi.mocked(mockBackend.execute).mockResolvedValue({
        type: 'SearchResults',
        data: mockResults,
      })

      const result = await api.searchWorkspace('test')

      expect(mockBackend.execute).toHaveBeenCalledWith({
        type: 'SearchWorkspace',
        params: {
          pattern: 'test',
          options: {
            workspace_path: null,
            search_frontmatter: false,
            property: null,
            case_sensitive: false,
          },
        },
      })
      expect(result).toEqual(mockResults)
    })

    it('should search workspace with custom options', async () => {
      vi.mocked(mockBackend.execute).mockResolvedValue({
        type: 'SearchResults',
        data: { files: [], files_searched: 5 },
      })

      await api.searchWorkspace('test', {
        workspace_path: 'docs',
        search_frontmatter: true,
        case_sensitive: true,
      })

      expect(mockBackend.execute).toHaveBeenCalledWith({
        type: 'SearchWorkspace',
        params: {
          pattern: 'test',
          options: {
            workspace_path: 'docs',
            search_frontmatter: true,
            property: null,
            case_sensitive: true,
          },
        },
      })
    })
  })

  describe('frontmatter operations', () => {
    it('should get frontmatter', async () => {
      const mockFrontmatter = { title: 'Test', author: 'User' }
      vi.mocked(mockBackend.execute).mockResolvedValue({
        type: 'Frontmatter',
        data: mockFrontmatter,
      })

      const result = await api.getFrontmatter('test.md')

      expect(mockBackend.execute).toHaveBeenCalledWith({
        type: 'GetFrontmatter',
        params: { path: 'test.md' },
      })
      expect(result).toEqual(mockFrontmatter)
    })

    it('should set frontmatter property', async () => {
      vi.mocked(mockBackend.execute).mockResolvedValue({ type: 'Ok' })

      await api.setFrontmatterProperty('test.md', 'title', 'New Title')

      expect(mockBackend.execute).toHaveBeenCalledWith({
        type: 'SetFrontmatterProperty',
        params: { path: 'test.md', key: 'title', value: 'New Title', root_index_path: null },
      })
      expect(browserPluginEventMocks.dispatchFileSavedEvent).toHaveBeenCalledWith('test.md', { bodyChanged: false })
      expect(workspaceMirrorMocks.mirrorCurrentWorkspaceMutationToLinkedProviders).toHaveBeenCalledTimes(1)
    })

    it('should dispatch file-saved for non-title frontmatter updates', async () => {
      vi.mocked(mockBackend.execute).mockResolvedValue({ type: 'Ok' })

      await api.setFrontmatterProperty('test.md', 'description', 'Updated description')

      expect(browserPluginEventMocks.dispatchFileSavedEvent).toHaveBeenCalledWith('test.md', { bodyChanged: false })
      expect(workspaceMirrorMocks.mirrorCurrentWorkspaceMutationToLinkedProviders).toHaveBeenCalledTimes(1)
    })

    it('should mirror frontmatter renames through plugin events', async () => {
      vi.mocked(mockBackend.execute).mockResolvedValue({
        type: 'String',
        data: 'renamed.md',
      })

      const result = await api.setFrontmatterProperty('test.md', 'title', 'Renamed', 'README.md')

      expect(result).toBe('renamed.md')
      expect(browserPluginEventMocks.dispatchFileMovedEvent).toHaveBeenCalledWith('test.md', 'renamed.md')
      expect(browserPluginEventMocks.dispatchFileSavedEvent).toHaveBeenCalledWith('renamed.md', { bodyChanged: false })
      expect(workspaceMirrorMocks.mirrorCurrentWorkspaceMutationToLinkedProviders).toHaveBeenCalledTimes(1)
    })

    it('should remove frontmatter property', async () => {
      vi.mocked(mockBackend.execute).mockResolvedValue({ type: 'Ok' })

      await api.removeFrontmatterProperty('test.md', 'author')

      expect(mockBackend.execute).toHaveBeenCalledWith({
        type: 'RemoveFrontmatterProperty',
        params: { path: 'test.md', key: 'author' },
      })
      expect(browserPluginEventMocks.dispatchFileSavedEvent).toHaveBeenCalledWith('test.md', { bodyChanged: false })
      expect(workspaceMirrorMocks.mirrorCurrentWorkspaceMutationToLinkedProviders).toHaveBeenCalledTimes(1)
    })
  })

  describe('attachment operations', () => {
    it('should get attachments', async () => {
      vi.mocked(mockBackend.execute).mockResolvedValue({
        type: 'Strings',
        data: ['image.png', 'doc.pdf'],
      })

      const result = await api.getAttachments('test.md')

      expect(mockBackend.execute).toHaveBeenCalledWith({
        type: 'GetAttachments',
        params: { path: 'test.md' },
      })
      expect(result).toEqual(['image.png', 'doc.pdf'])
    })

    it('should upload attachment', async () => {
      vi.mocked(mockBackend.execute)
        .mockResolvedValueOnce({
          type: 'String',
          data: '/workspace/_attachments/image.png',
        })
        .mockResolvedValueOnce({
          type: 'String',
          data: '_attachments/image.png',
        })

      const bytes = new Uint8Array([1, 2, 3])
      const result = await api.uploadAttachment('test.md', 'image.png', bytes)

      expect(mockBackend.execute).toHaveBeenNthCalledWith(1, {
        type: 'ResolveAttachmentPath',
        params: {
          entry_path: 'test.md',
          attachment_path: '_attachments/image.png',
        },
      })
      expect(mockBackend.writeBinary).toHaveBeenCalledWith('/workspace/_attachments/image.png', bytes)
      expect(mockBackend.execute).toHaveBeenNthCalledWith(2, {
        type: 'RegisterAttachment',
        params: {
          entry_path: 'test.md',
          filename: 'image.png',
        },
      })
      expect(result).toBe('_attachments/image.png')
    })

    it('should delete attachment', async () => {
      vi.mocked(mockBackend.execute).mockResolvedValue({
        type: 'Ok',
      })

      await api.deleteAttachment('test.md', 'image.png')

      expect(mockBackend.execute).toHaveBeenCalledWith({
        type: 'DeleteAttachment',
        params: { entry_path: 'test.md', attachment_path: 'image.png' },
      })
    })

    it('should get attachment data', async () => {
      vi.mocked(mockBackend.execute).mockResolvedValue({
        type: 'Bytes',
        data: [1, 2, 3, 4],
      })

      const result = await api.getAttachmentData('test.md', 'image.png')

      expect(mockBackend.execute).toHaveBeenCalledWith({
        type: 'GetAttachmentData',
        params: { entry_path: 'test.md', attachment_path: 'image.png' },
      })
      expect(result).toEqual([1, 2, 3, 4])
    })
  })

  describe('template operations', () => {
    it('should list templates via plugin command', async () => {
      const mockTemplates = [
        { name: 'note', path: 'templates/note.md', source: 'workspace' },
      ]
      vi.mocked(mockBackend.execute).mockResolvedValue({
        type: 'PluginResult',
        data: mockTemplates,
      })

      const result = await api.listTemplates()

      expect(mockBackend.execute).toHaveBeenCalledWith({
        type: 'PluginCommand',
        params: { plugin: 'diaryx.templating', command: 'ListTemplates', params: { workspace_path: null } },
      })
      expect(result).toEqual(mockTemplates)
    })

    it('should get template content via plugin command', async () => {
      vi.mocked(mockBackend.execute).mockResolvedValue({
        type: 'PluginResult',
        data: '# Note\n\n{{content}}',
      })

      const result = await api.getTemplate('note')

      expect(mockBackend.execute).toHaveBeenCalledWith({
        type: 'PluginCommand',
        params: { plugin: 'diaryx.templating', command: 'GetTemplate', params: { name: 'note', workspace_path: null } },
      })
      expect(result).toBe('# Note\n\n{{content}}')
    })
  })

  describe('file operations', () => {
    it('should check if file exists', async () => {
      vi.mocked(mockBackend.execute).mockResolvedValue({
        type: 'Bool',
        data: true,
      })

      const result = await api.fileExists('test.md')

      expect(mockBackend.execute).toHaveBeenCalledWith({
        type: 'FileExists',
        params: { path: 'test.md' },
      })
      expect(result).toBe(true)
    })

    it('should read file content', async () => {
      vi.mocked(mockBackend.execute).mockResolvedValue({
        type: 'String',
        data: '# Hello World',
      })

      const result = await api.readFile('test.md')

      expect(mockBackend.execute).toHaveBeenCalledWith({
        type: 'ReadFile',
        params: { path: 'test.md' },
      })
      expect(result).toBe('# Hello World')
    })

    it('should write file content', async () => {
      vi.mocked(mockBackend.execute).mockResolvedValue({ type: 'Ok' })

      await api.writeFile('test.md', '# New Content')

      expect(mockBackend.execute).toHaveBeenCalledWith({
        type: 'WriteFile',
        params: { path: 'test.md', content: '# New Content' },
      })
    })

    it('should delete file', async () => {
      vi.mocked(mockBackend.execute).mockResolvedValue({ type: 'Ok' })

      await api.deleteFile('test.md')

      expect(mockBackend.execute).toHaveBeenCalledWith({
        type: 'DeleteFile',
        params: { path: 'test.md' },
      })
    })

    it('should read binary file', async () => {
      const mockData = new Uint8Array([1, 2, 3])
      vi.mocked(mockBackend.readBinary).mockResolvedValue(mockData)

      const result = await api.readBinary('image.png')

      expect(mockBackend.readBinary).toHaveBeenCalledWith('image.png')
      expect(result).toBe(mockData)
    })

    it('should write binary file', async () => {
      const data = new Uint8Array([1, 2, 3])
      vi.mocked(mockBackend.writeBinary).mockResolvedValue(undefined)

      await api.writeBinary('image.png', data)

      expect(mockBackend.writeBinary).toHaveBeenCalledWith('image.png', data)
    })
  })

  describe('storage operations', () => {
    it('should get storage usage', async () => {
      const mockInfo = { used: BigInt(1024), limit: BigInt(10240), attachment_limit: BigInt(5120) }
      vi.mocked(mockBackend.execute).mockResolvedValue({
        type: 'StorageInfo',
        data: mockInfo,
      })

      const result = await api.getStorageUsage()

      expect(mockBackend.execute).toHaveBeenCalledWith({ type: 'GetStorageUsage' })
      expect(result).toEqual(mockInfo)
    })
  })
})
