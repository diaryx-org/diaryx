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

const permissionStoreMocks = vi.hoisted(() => ({
  permissionStore: {
    requestPermission: vi.fn().mockResolvedValue(true),
  },
}))

vi.mock('$lib/plugins/browserPluginManager.svelte', () => browserPluginEventMocks)
vi.mock('$lib/sync/browserWorkspaceMutationMirror', () => workspaceMirrorMocks)
vi.mock('@/models/stores/permissionStore.svelte', () => permissionStoreMocks)

import { createApi } from './api'
import { BackendError, type Backend } from './interface'

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
      revealInFileManager: vi.fn().mockResolvedValue(undefined),
      importFromZip: vi.fn().mockResolvedValue({ success: true, files_imported: 0 }),
    }
    api = createApi(mockBackend)
    vi.clearAllMocks()
    permissionStoreMocks.permissionStore.requestPermission.mockResolvedValue(true)
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

  describe('getPluginComponentHtml', () => {
    it('prefers the backend direct component-html path when available', async () => {
      mockBackend.getPluginComponentHtml = vi
        .fn()
        .mockResolvedValue('<section>Daily</section>')

      const result = await api.getPluginComponentHtml('diaryx.daily', 'daily.panel')

      expect(mockBackend.getPluginComponentHtml).toHaveBeenCalledWith(
        'diaryx.daily',
        'daily.panel',
      )
      expect(mockBackend.execute).not.toHaveBeenCalled()
      expect(result).toBe('<section>Daily</section>')
    })

    it('falls back to PluginCommand when the backend direct path is unavailable', async () => {
      vi.mocked(mockBackend.execute).mockResolvedValue({
        type: 'PluginResult',
        data: '<section>Daily</section>',
      } as any)

      const result = await api.getPluginComponentHtml('diaryx.daily', 'daily.panel')

      expect(mockBackend.execute).toHaveBeenCalledWith({
        type: 'PluginCommand',
        params: {
          plugin: 'diaryx.daily',
          command: 'get_component_html',
          params: { component_id: 'daily.panel' },
        },
      })
      expect(result).toBe('<section>Daily</section>')
    })

    it('retries native component rendering after approving a Tauri permission request', async () => {
      mockBackend.getWorkspacePath = vi.fn().mockReturnValue('/Users/test/journal/README.md')
      mockBackend.getPluginComponentHtml = vi
        .fn()
        .mockRejectedValueOnce(
          new BackendError(
            "Permission not configured for plugin 'diaryx.daily': read_files on '/Users/test/journal/Daily/daily_index.md'",
            'PluginError',
          ),
        )
        .mockResolvedValueOnce('<section>Daily</section>')

      const result = await api.getPluginComponentHtml('diaryx.daily', 'daily.panel')

      expect(permissionStoreMocks.permissionStore.requestPermission).toHaveBeenCalledWith(
        'diaryx.daily',
        'diaryx.daily',
        'read_files',
        'Daily/daily_index.md',
      )
      expect(mockBackend.getPluginComponentHtml).toHaveBeenCalledTimes(2)
      expect(result).toBe('<section>Daily</section>')
    })
  })

  describe('executePluginCommand', () => {
    it('retries a Tauri plugin command after the permission banner allows it', async () => {
      mockBackend.getWorkspacePath = vi.fn().mockReturnValue('/Users/test/journal/README.md')
      vi.mocked(mockBackend.execute)
        .mockRejectedValueOnce(
          new BackendError(
            "Permission not configured for plugin 'diaryx.daily': read_files on '/Users/test/journal/README.md'",
            'PluginError',
          ),
        )
        .mockResolvedValueOnce({
          type: 'PluginResult',
          data: { ok: true },
        } as any)

      const result = await api.executePluginCommand('diaryx.daily', 'OpenToday', {})

      expect(permissionStoreMocks.permissionStore.requestPermission).toHaveBeenCalledWith(
        'diaryx.daily',
        'diaryx.daily',
        'read_files',
        'README.md',
      )
      expect(mockBackend.execute).toHaveBeenCalledTimes(2)
      expect(result).toEqual({ ok: true })
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
        is_index: false,
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
        data: { path: 'subdir', name: 'subdir', description: null, is_index: false, children: [], properties: {} },
      })

      await api.getWorkspaceTree('subdir', 2)

      expect(mockBackend.execute).toHaveBeenCalledWith({
        type: 'GetWorkspaceTree',
        params: { path: 'subdir', depth: 2, audience: null },
      })
    })
  })

  describe('resolveWorkspaceRootIndexPath', () => {
    it('returns a preferred root index file path without calling FindRootIndex', async () => {
      const result = await api.resolveWorkspaceRootIndexPath('/Users/test/journal/README.md')

      expect(mockBackend.execute).not.toHaveBeenCalled()
      expect(result).toBe('/Users/test/journal/README.md')
    })

    it('resolves a workspace directory through FindRootIndex', async () => {
      vi.mocked(mockBackend.execute).mockResolvedValue({
        type: 'String',
        data: '/Users/test/journal/README.md',
      })

      const result = await api.resolveWorkspaceRootIndexPath('/Users/test/journal/')

      expect(mockBackend.execute).toHaveBeenCalledWith({
        type: 'FindRootIndex',
        params: { directory: '/Users/test/journal' },
      })
      expect(result).toBe('/Users/test/journal/README.md')
    })

    it('falls back to the backend workspace path when the preferred directory lookup fails', async () => {
      mockBackend.getWorkspacePath = vi.fn().mockReturnValue('/Users/test/journal/README.md')
      vi.mocked(mockBackend.execute).mockRejectedValueOnce(new Error('workspace root not found'))

      const result = await api.resolveWorkspaceRootIndexPath('/Users/test/journal/')

      expect(result).toBe('/Users/test/journal/README.md')
      expect(mockBackend.execute).toHaveBeenCalledTimes(1)
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

    it('should reveal a workspace item in the file manager', async () => {
      await api.revealInFileManager('notes/today.md')

      expect(mockBackend.revealInFileManager).toHaveBeenCalledWith('notes/today.md')
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

  describe('duplicateEntry', () => {
    it('should duplicate entry and dispatch created event', async () => {
      vi.mocked(mockBackend.execute).mockResolvedValue({
        type: 'String',
        data: 'copy-of-test.md',
      })

      const result = await api.duplicateEntry('test.md')

      expect(mockBackend.execute).toHaveBeenCalledWith({
        type: 'DuplicateEntry',
        params: { path: 'test.md' },
      })
      expect(result).toBe('copy-of-test.md')
      expect(browserPluginEventMocks.dispatchFileCreatedEvent).toHaveBeenCalledWith('copy-of-test.md')
      expect(workspaceMirrorMocks.mirrorCurrentWorkspaceMutationToLinkedProviders).toHaveBeenCalledTimes(1)
    })
  })

  describe('convertToIndex', () => {
    it('should convert leaf to index and return new path', async () => {
      vi.mocked(mockBackend.execute).mockResolvedValue({
        type: 'String',
        data: 'folder/README.md',
      })

      const result = await api.convertToIndex('folder.md')

      expect(mockBackend.execute).toHaveBeenCalledWith({
        type: 'ConvertToIndex',
        params: { path: 'folder.md' },
      })
      expect(result).toBe('folder/README.md')
      expect(workspaceMirrorMocks.mirrorCurrentWorkspaceMutationToLinkedProviders).toHaveBeenCalledTimes(1)
    })
  })

  describe('convertToLeaf', () => {
    it('should convert index to leaf and return new path', async () => {
      vi.mocked(mockBackend.execute).mockResolvedValue({
        type: 'String',
        data: 'folder.md',
      })

      const result = await api.convertToLeaf('folder/README.md')

      expect(mockBackend.execute).toHaveBeenCalledWith({
        type: 'ConvertToLeaf',
        params: { path: 'folder/README.md' },
      })
      expect(result).toBe('folder.md')
      expect(workspaceMirrorMocks.mirrorCurrentWorkspaceMutationToLinkedProviders).toHaveBeenCalledTimes(1)
    })
  })

  describe('createChildEntry', () => {
    it('should create child entry and dispatch created event', async () => {
      const mockResult = {
        child_path: 'parent/child.md',
        parent_path: 'parent/README.md',
        parent_converted: false,
        original_parent_path: undefined,
      }
      vi.mocked(mockBackend.execute).mockResolvedValue({
        type: 'CreateChildResult',
        data: mockResult,
      })

      const result = await api.createChildEntry('parent/README.md')

      expect(mockBackend.execute).toHaveBeenCalledWith({
        type: 'CreateChildEntry',
        params: { parent_path: 'parent/README.md' },
      })
      expect(result).toEqual(mockResult)
      expect(browserPluginEventMocks.dispatchFileCreatedEvent).toHaveBeenCalledWith('parent/child.md')
      expect(browserPluginEventMocks.dispatchFileMovedEvent).not.toHaveBeenCalled()
      expect(workspaceMirrorMocks.mirrorCurrentWorkspaceMutationToLinkedProviders).toHaveBeenCalledTimes(1)
    })

    it('should dispatch move event when parent was converted', async () => {
      const mockResult = {
        child_path: 'parent/child.md',
        parent_path: 'parent/README.md',
        parent_converted: true,
        original_parent_path: 'parent.md',
      }
      vi.mocked(mockBackend.execute).mockResolvedValue({
        type: 'CreateChildResult',
        data: mockResult,
      })

      const result = await api.createChildEntry('parent.md')

      expect(result).toEqual(mockResult)
      expect(browserPluginEventMocks.dispatchFileCreatedEvent).toHaveBeenCalledWith('parent/child.md')
      expect(browserPluginEventMocks.dispatchFileMovedEvent).toHaveBeenCalledWith('parent.md', 'parent/README.md')
      expect(workspaceMirrorMocks.mirrorCurrentWorkspaceMutationToLinkedProviders).toHaveBeenCalledTimes(1)
    })
  })

  describe('findRootIndex', () => {
    it('should find root index in a directory', async () => {
      vi.mocked(mockBackend.execute).mockResolvedValue({
        type: 'String',
        data: '/Users/test/journal/README.md',
      })

      const result = await api.findRootIndex('/Users/test/journal')

      expect(mockBackend.execute).toHaveBeenCalledWith({
        type: 'FindRootIndex',
        params: { directory: '/Users/test/journal' },
      })
      expect(result).toBe('/Users/test/journal/README.md')
    })
  })

  describe('getFilesystemTree', () => {
    it('should get filesystem tree with defaults', async () => {
      const mockTree = { path: 'root', name: 'root', description: null, is_index: false, children: [], properties: {} }
      vi.mocked(mockBackend.execute).mockResolvedValue({
        type: 'Tree',
        data: mockTree,
      })

      const result = await api.getFilesystemTree()

      expect(mockBackend.execute).toHaveBeenCalledWith({
        type: 'GetFilesystemTree',
        params: { path: null, show_hidden: false, depth: null },
      })
      expect(result).toEqual(mockTree)
    })

    it('should pass options through', async () => {
      vi.mocked(mockBackend.execute).mockResolvedValue({
        type: 'Tree',
        data: { path: 'root', name: 'root', description: null, is_index: false, children: [], properties: {} },
      })

      await api.getFilesystemTree('subdir', true, 3)

      expect(mockBackend.execute).toHaveBeenCalledWith({
        type: 'GetFilesystemTree',
        params: { path: 'subdir', show_hidden: true, depth: 3 },
      })
    })
  })

  describe('createWorkspace', () => {
    it('should create workspace with defaults', async () => {
      vi.mocked(mockBackend.execute).mockResolvedValue({ type: 'Ok' })

      await api.createWorkspace()

      expect(mockBackend.execute).toHaveBeenCalledWith({
        type: 'CreateWorkspace',
        params: { path: null, name: null },
      })
    })

    it('should create workspace with path and name', async () => {
      vi.mocked(mockBackend.execute).mockResolvedValue({ type: 'Ok' })

      await api.createWorkspace('/tmp/journal', 'My Journal')

      expect(mockBackend.execute).toHaveBeenCalledWith({
        type: 'CreateWorkspace',
        params: { path: '/tmp/journal', name: 'My Journal' },
      })
    })
  })

  describe('getWorkspaceConfig', () => {
    it('should get workspace config', async () => {
      const mockConfig = {
        filename_style: 'kebab_case' as const,
        link_format: 'markdown_root' as const,
        sync_title_to_heading: false,
        auto_update_timestamp: false,
        auto_rename_to_title: false,
      }
      vi.mocked(mockBackend.execute).mockResolvedValue({
        type: 'WorkspaceConfig',
        data: mockConfig,
      })

      const result = await api.getWorkspaceConfig('README.md')

      expect(mockBackend.execute).toHaveBeenCalledWith({
        type: 'GetWorkspaceConfig',
        params: { root_index_path: 'README.md' },
      })
      expect(result).toEqual(mockConfig)
    })
  })

  describe('generateFilename', () => {
    it('should generate filename from title', async () => {
      vi.mocked(mockBackend.execute).mockResolvedValue({
        type: 'String',
        data: 'my-new-entry.md',
      })

      const result = await api.generateFilename('My New Entry')

      expect(mockBackend.execute).toHaveBeenCalledWith({
        type: 'GenerateFilename',
        params: { title: 'My New Entry', root_index_path: null },
      })
      expect(result).toBe('my-new-entry.md')
    })

    it('should pass root index path', async () => {
      vi.mocked(mockBackend.execute).mockResolvedValue({
        type: 'String',
        data: 'my-new-entry.md',
      })

      await api.generateFilename('My New Entry', 'README.md')

      expect(mockBackend.execute).toHaveBeenCalledWith({
        type: 'GenerateFilename',
        params: { title: 'My New Entry', root_index_path: 'README.md' },
      })
    })
  })

  describe('setWorkspaceConfig', () => {
    it('should set workspace config and mirror', async () => {
      vi.mocked(mockBackend.execute).mockResolvedValue({ type: 'Ok' })

      await api.setWorkspaceConfig('README.md', 'filename_style', 'kebab-case')

      expect(mockBackend.execute).toHaveBeenCalledWith({
        type: 'SetWorkspaceConfig',
        params: { root_index_path: 'README.md', field: 'filename_style', value: 'kebab-case' },
      })
      expect(workspaceMirrorMocks.mirrorCurrentWorkspaceMutationToLinkedProviders).toHaveBeenCalledTimes(1)
    })
  })

  describe('reorderFrontmatterKeys', () => {
    it('should reorder keys and dispatch events', async () => {
      vi.mocked(mockBackend.execute).mockResolvedValue({ type: 'Ok' })

      await api.reorderFrontmatterKeys('test.md', ['title', 'author', 'date'])

      expect(mockBackend.execute).toHaveBeenCalledWith({
        type: 'ReorderFrontmatterKeys',
        params: { path: 'test.md', keys: ['title', 'author', 'date'] },
      })
      expect(browserPluginEventMocks.dispatchFileSavedEvent).toHaveBeenCalledWith('test.md', { bodyChanged: false })
      expect(workspaceMirrorMocks.mirrorCurrentWorkspaceMutationToLinkedProviders).toHaveBeenCalledTimes(1)
    })
  })

  describe('moveFrontmatterSectionToFile', () => {
    it('should move section and dispatch events', async () => {
      vi.mocked(mockBackend.execute).mockResolvedValue({ type: 'Ok' })

      await api.moveFrontmatterSectionToFile('source.md', 'config', 'target.md', true)

      expect(mockBackend.execute).toHaveBeenCalledWith({
        type: 'MoveFrontmatterSectionToFile',
        params: {
          source_path: 'source.md',
          section_key: 'config',
          target_path: 'target.md',
          create_if_missing: true,
        },
      })
      expect(browserPluginEventMocks.dispatchFileSavedEvent).toHaveBeenCalledWith('source.md', { bodyChanged: false })
      expect(workspaceMirrorMocks.mirrorCurrentWorkspaceMutationToLinkedProviders).toHaveBeenCalledTimes(1)
    })

    it('should default createIfMissing to true', async () => {
      vi.mocked(mockBackend.execute).mockResolvedValue({ type: 'Ok' })

      await api.moveFrontmatterSectionToFile('source.md', 'config', 'target.md')

      expect(mockBackend.execute).toHaveBeenCalledWith({
        type: 'MoveFrontmatterSectionToFile',
        params: {
          source_path: 'source.md',
          section_key: 'config',
          target_path: 'target.md',
          create_if_missing: true,
        },
      })
    })
  })

  describe('link parser operations', () => {
    it('should run a link parser operation', async () => {
      vi.mocked(mockBackend.execute).mockResolvedValue({
        type: 'LinkParserResult',
        data: { type: 'parsed', data: { title: 'Test', path: 'test.md', path_type: 'workspace_root' } },
      } as any)

      const result = await api.runLinkParser({ type: 'parse', params: { link: '[[Test]]' } })

      expect(mockBackend.execute).toHaveBeenCalledWith({
        type: 'LinkParser',
        params: { operation: { type: 'parse', params: { link: '[[Test]]' } } },
      })
      expect(result).toEqual({ type: 'parsed', data: { title: 'Test', path: 'test.md', path_type: 'workspace_root' } })
    })

    it('should throw on unexpected response type from runLinkParser', async () => {
      vi.mocked(mockBackend.execute).mockResolvedValue({ type: 'Ok' })

      await expect(api.runLinkParser({ type: 'parse', params: { link: '[[Test]]' } })).rejects.toThrow(
        "Expected response type 'LinkParserResult', got 'Ok'"
      )
    })

    it('should parse a link', async () => {
      vi.mocked(mockBackend.execute).mockResolvedValue({
        type: 'LinkParserResult',
        data: { type: 'parsed', data: { title: 'My Page', path: 'my-page.md', path_type: 'workspace_root' } },
      } as any)

      const result = await api.parseLink('[[My Page]]')
      expect(result).toEqual({ title: 'My Page', path: 'my-page.md', path_type: 'workspace_root' })
    })

    it('should throw when parseLink gets non-parsed result', async () => {
      vi.mocked(mockBackend.execute).mockResolvedValue({
        type: 'LinkParserResult',
        data: { type: 'string', data: 'something' },
      } as any)

      await expect(api.parseLink('[[Test]]')).rejects.toThrow(
        "Expected link parser result type 'parsed', got 'string'"
      )
    })

    it('should canonicalize a link', async () => {
      vi.mocked(mockBackend.execute).mockResolvedValue({
        type: 'LinkParserResult',
        data: { type: 'string', data: 'folder/page.md' },
      } as any)

      const result = await api.canonicalizeLink('./page.md', 'folder/index.md')
      expect(result).toBe('folder/page.md')
    })

    it('should throw when canonicalizeLink gets non-string result', async () => {
      vi.mocked(mockBackend.execute).mockResolvedValue({
        type: 'LinkParserResult',
        data: { type: 'parsed', data: { title: null, path: 'x', path_type: 'relative' } },
      } as any)

      await expect(api.canonicalizeLink('./page.md', 'folder/index.md')).rejects.toThrow(
        "Expected link parser result type 'string', got 'parsed'"
      )
    })

    it('should format a link', async () => {
      vi.mocked(mockBackend.execute).mockResolvedValue({
        type: 'LinkParserResult',
        data: { type: 'string', data: '[[My Page]]' },
      } as any)

      const result = await api.formatLink('my-page.md', 'My Page', 'WikiLink' as any, 'index.md')
      expect(result).toBe('[[My Page]]')
    })

    it('should throw when formatLink gets non-string result', async () => {
      vi.mocked(mockBackend.execute).mockResolvedValue({
        type: 'LinkParserResult',
        data: { type: 'parsed', data: { title: null, path: 'x', path_type: 'relative' } },
      } as any)

      await expect(api.formatLink('x.md', 'X', 'WikiLink' as any, 'index.md')).rejects.toThrow(
        "Expected link parser result type 'string', got 'parsed'"
      )
    })

    it('should convert a link', async () => {
      vi.mocked(mockBackend.execute).mockResolvedValue({
        type: 'LinkParserResult',
        data: { type: 'string', data: '[Page](./page.md)' },
      } as any)

      const result = await api.convertLink('[[Page]]', 'Markdown' as any, 'folder/index.md')
      expect(result).toBe('[Page](./page.md)')
    })

    it('should throw when convertLink gets non-string result', async () => {
      vi.mocked(mockBackend.execute).mockResolvedValue({
        type: 'LinkParserResult',
        data: { type: 'parsed', data: { title: null, path: 'x', path_type: 'relative' } },
      } as any)

      await expect(api.convertLink('[[Page]]', 'Markdown' as any, 'index.md')).rejects.toThrow(
        "Expected link parser result type 'string', got 'parsed'"
      )
    })
  })

  describe('validation operations', () => {
    it('should validate a single file', async () => {
      const mockResult = { errors: [], warnings: [], files_checked: 1 }
      vi.mocked(mockBackend.execute).mockResolvedValue({
        type: 'ValidationResult',
        data: mockResult,
      })

      const result = await api.validateFile('test.md')

      expect(mockBackend.execute).toHaveBeenCalledWith({
        type: 'ValidateFile',
        params: { path: 'test.md' },
      })
      expect(result).toEqual(mockResult)
    })

    it('should validate workspace with a specific path', async () => {
      const mockResult = { errors: [], warnings: [], files_checked: 5 }
      vi.mocked(mockBackend.execute).mockResolvedValue({
        type: 'ValidationResult',
        data: mockResult,
      })

      const result = await api.validateWorkspace('subdir')

      expect(mockBackend.execute).toHaveBeenCalledWith({
        type: 'ValidateWorkspace',
        params: { path: 'subdir' },
      })
      expect(result).toEqual(mockResult)
    })

    it('should fix broken part_of', async () => {
      const mockFix = { success: true, message: 'Fixed' }
      vi.mocked(mockBackend.execute).mockResolvedValue({
        type: 'FixResult',
        data: mockFix,
      })

      const result = await api.fixBrokenPartOf('broken.md')

      expect(mockBackend.execute).toHaveBeenCalledWith({
        type: 'FixBrokenPartOf',
        params: { path: 'broken.md' },
      })
      expect(result).toEqual(mockFix)
    })

    it('should fix broken contents ref', async () => {
      const mockFix = { success: true, message: 'Fixed' }
      vi.mocked(mockBackend.execute).mockResolvedValue({
        type: 'FixResult',
        data: mockFix,
      })

      const result = await api.fixBrokenContentsRef('index.md', 'missing.md')

      expect(mockBackend.execute).toHaveBeenCalledWith({
        type: 'FixBrokenContentsRef',
        params: { index_path: 'index.md', target: 'missing.md' },
      })
      expect(result).toEqual(mockFix)
    })

    it('should fix broken attachment', async () => {
      const mockFix = { success: true, message: 'Fixed' }
      vi.mocked(mockBackend.execute).mockResolvedValue({
        type: 'FixResult',
        data: mockFix,
      })

      const result = await api.fixBrokenAttachment('test.md', 'missing.png')

      expect(mockBackend.execute).toHaveBeenCalledWith({
        type: 'FixBrokenAttachment',
        params: { path: 'test.md', attachment: 'missing.png' },
      })
      expect(result).toEqual(mockFix)
    })

    it('should fix non-portable path', async () => {
      const mockFix = { success: true, message: 'Fixed' }
      vi.mocked(mockBackend.execute).mockResolvedValue({
        type: 'FixResult',
        data: mockFix,
      })

      const result = await api.fixNonPortablePath('test.md', 'part_of', 'Old Name.md', 'old-name.md')

      expect(mockBackend.execute).toHaveBeenCalledWith({
        type: 'FixNonPortablePath',
        params: { path: 'test.md', property: 'part_of', old_value: 'Old Name.md', new_value: 'old-name.md' },
      })
      expect(result).toEqual(mockFix)
    })

    it('should fix unlisted file', async () => {
      const mockFix = { success: true, message: 'Fixed' }
      vi.mocked(mockBackend.execute).mockResolvedValue({
        type: 'FixResult',
        data: mockFix,
      })

      const result = await api.fixUnlistedFile('index.md', 'unlisted.md')

      expect(mockBackend.execute).toHaveBeenCalledWith({
        type: 'FixUnlistedFile',
        params: { index_path: 'index.md', file_path: 'unlisted.md' },
      })
      expect(result).toEqual(mockFix)
    })

    it('should fix orphan binary file', async () => {
      const mockFix = { success: true, message: 'Fixed' }
      vi.mocked(mockBackend.execute).mockResolvedValue({
        type: 'FixResult',
        data: mockFix,
      })

      const result = await api.fixOrphanBinaryFile('index.md', 'orphan.png')

      expect(mockBackend.execute).toHaveBeenCalledWith({
        type: 'FixOrphanBinaryFile',
        params: { index_path: 'index.md', file_path: 'orphan.png' },
      })
      expect(result).toEqual(mockFix)
    })

    it('should fix missing part_of', async () => {
      const mockFix = { success: true, message: 'Fixed' }
      vi.mocked(mockBackend.execute).mockResolvedValue({
        type: 'FixResult',
        data: mockFix,
      })

      const result = await api.fixMissingPartOf('child.md', 'index.md')

      expect(mockBackend.execute).toHaveBeenCalledWith({
        type: 'FixMissingPartOf',
        params: { file_path: 'child.md', index_path: 'index.md' },
      })
      expect(result).toEqual(mockFix)
    })

    it('should fix all validation issues', async () => {
      const mockSummary = { error_fixes: [], warning_fixes: [], total_fixed: 5, total_failed: 0 }
      const mockValidation = { errors: [], warnings: [] } as any
      vi.mocked(mockBackend.execute).mockResolvedValue({
        type: 'FixSummary',
        data: mockSummary,
      })

      const result = await api.fixAll(mockValidation)

      expect(mockBackend.execute).toHaveBeenCalledWith({
        type: 'FixAll',
        params: { validation_result: mockValidation },
      })
      expect(result).toEqual(mockSummary)
    })

    it('should fix circular reference', async () => {
      const mockFix = { success: true, message: 'Fixed' }
      vi.mocked(mockBackend.execute).mockResolvedValue({
        type: 'FixResult',
        data: mockFix,
      })

      const result = await api.fixCircularReference('a.md', 'b.md')

      expect(mockBackend.execute).toHaveBeenCalledWith({
        type: 'FixCircularReference',
        params: { file_path: 'a.md', part_of_value: 'b.md' },
      })
      expect(result).toEqual(mockFix)
    })

    it('should get available parent indexes', async () => {
      vi.mocked(mockBackend.execute).mockResolvedValue({
        type: 'Strings',
        data: ['index.md', 'folder/README.md'],
      })

      const result = await api.getAvailableParentIndexes('orphan.md', 'README.md')

      expect(mockBackend.execute).toHaveBeenCalledWith({
        type: 'GetAvailableParentIndexes',
        params: { file_path: 'orphan.md', workspace_root: 'README.md' },
      })
      expect(result).toEqual(['index.md', 'folder/README.md'])
    })
  })

  describe('export operations', () => {
    it('should get effective audience', async () => {
      const mockResult = { audience: ['public'], inherited: false }
      vi.mocked(mockBackend.execute).mockResolvedValue({
        type: 'EffectiveAudienceResult',
        data: mockResult,
      } as any)

      const result = await api.getEffectiveAudience('test.md')

      expect(mockBackend.execute).toHaveBeenCalledWith({
        type: 'GetEffectiveAudience',
        params: { path: 'test.md' },
      })
      expect(result).toEqual(mockResult)
    })

    it('should get available audiences', async () => {
      vi.mocked(mockBackend.execute).mockResolvedValue({
        type: 'Strings',
        data: ['public', 'team'],
      })

      const result = await api.getAvailableAudiences('README.md')

      expect(mockBackend.execute).toHaveBeenCalledWith({
        type: 'GetAvailableAudiences',
        params: { path: 'README.md' },
      })
      expect(result).toEqual(['public', 'team'])
    })

    it('should export to HTML via plugin command', async () => {
      vi.mocked(mockBackend.execute).mockResolvedValue({
        type: 'PluginResult',
        data: [{ path: 'index.html', content: '<h1>Test</h1>' }],
      })

      const result = await api.exportToHtml('README.md', 'public')

      expect(mockBackend.execute).toHaveBeenCalledWith({
        type: 'PluginCommand',
        params: {
          plugin: 'publish',
          command: 'ExportToHtml',
          params: { root_path: 'README.md', audience: 'public' },
        },
      })
      expect(result).toEqual([{ path: 'index.html', content: '<h1>Test</h1>' }])
    })
  })

  describe('additional template operations', () => {
    it('should save template', async () => {
      vi.mocked(mockBackend.execute).mockResolvedValue({
        type: 'PluginResult',
        data: null,
      })

      await api.saveTemplate('note', '# Note\n\n{{content}}', '/workspace')

      expect(mockBackend.execute).toHaveBeenCalledWith({
        type: 'PluginCommand',
        params: {
          plugin: 'diaryx.templating',
          command: 'SaveTemplate',
          params: { name: 'note', content: '# Note\n\n{{content}}', workspace_path: '/workspace' },
        },
      })
    })

    it('should delete template', async () => {
      vi.mocked(mockBackend.execute).mockResolvedValue({
        type: 'PluginResult',
        data: null,
      })

      await api.deleteTemplate('note', '/workspace')

      expect(mockBackend.execute).toHaveBeenCalledWith({
        type: 'PluginCommand',
        params: {
          plugin: 'diaryx.templating',
          command: 'DeleteTemplate',
          params: { name: 'note', workspace_path: '/workspace' },
        },
      })
    })
  })

  describe('additional attachment operations', () => {
    it('should resolve attachment storage path', async () => {
      vi.mocked(mockBackend.execute).mockResolvedValue({
        type: 'String',
        data: '/workspace/_attachments/image.png',
      })

      const result = await api.resolveAttachmentStoragePath('test.md', '_attachments/image.png')

      expect(mockBackend.execute).toHaveBeenCalledWith({
        type: 'ResolveAttachmentPath',
        params: { entry_path: 'test.md', attachment_path: '_attachments/image.png' },
      })
      expect(result).toBe('/workspace/_attachments/image.png')
    })

    it('should move attachment', async () => {
      vi.mocked(mockBackend.execute).mockResolvedValue({
        type: 'String',
        data: '_attachments/moved.png',
      })

      const result = await api.moveAttachment('source.md', 'target.md', '_attachments/image.png', 'moved.png')

      expect(mockBackend.execute).toHaveBeenCalledWith({
        type: 'MoveAttachment',
        params: {
          source_entry_path: 'source.md',
          target_entry_path: 'target.md',
          attachment_path: '_attachments/image.png',
          new_filename: 'moved.png',
        },
      })
      expect(result).toBe('_attachments/moved.png')
    })

    it('should move attachment without new filename', async () => {
      vi.mocked(mockBackend.execute).mockResolvedValue({
        type: 'String',
        data: '_attachments/image.png',
      })

      await api.moveAttachment('source.md', 'target.md', '_attachments/image.png')

      expect(mockBackend.execute).toHaveBeenCalledWith({
        type: 'MoveAttachment',
        params: {
          source_entry_path: 'source.md',
          target_entry_path: 'target.md',
          attachment_path: '_attachments/image.png',
          new_filename: null,
        },
      })
    })

    it('should get ancestor attachments', async () => {
      const mockResult = { entries: [{ entry_path: 'index.md', entry_title: null, attachments: ['logo.png'] }] }
      vi.mocked(mockBackend.execute).mockResolvedValue({
        type: 'AncestorAttachments',
        data: mockResult,
      })

      const result = await api.getAncestorAttachments('child.md')

      expect(mockBackend.execute).toHaveBeenCalledWith({
        type: 'GetAncestorAttachments',
        params: { path: 'child.md' },
      })
      expect(result).toEqual(mockResult)
    })
  })

  describe('additional file operations', () => {
    it('should write file with metadata', async () => {
      vi.mocked(mockBackend.execute).mockResolvedValue({ type: 'Ok' })

      await api.writeFileWithMetadata('test.md', { title: 'Test' }, '# Body content')

      expect(mockBackend.execute).toHaveBeenCalledWith({
        type: 'WriteFileWithMetadata',
        params: { path: 'test.md', metadata: { title: 'Test' }, body: '# Body content' },
      })
    })

    it('should update file metadata with body', async () => {
      vi.mocked(mockBackend.execute).mockResolvedValue({ type: 'Ok' })

      await api.updateFileMetadata('test.md', { title: 'Updated' }, '# New body')

      expect(mockBackend.execute).toHaveBeenCalledWith({
        type: 'UpdateFileMetadata',
        params: { path: 'test.md', metadata: { title: 'Updated' }, body: '# New body' },
      })
    })

    it('should update file metadata without body', async () => {
      vi.mocked(mockBackend.execute).mockResolvedValue({ type: 'Ok' })

      await api.updateFileMetadata('test.md', { title: 'Updated' })

      expect(mockBackend.execute).toHaveBeenCalledWith({
        type: 'UpdateFileMetadata',
        params: { path: 'test.md', metadata: { title: 'Updated' }, body: null },
      })
    })

    it('should throw when revealInFileManager is not supported', async () => {
      mockBackend.revealInFileManager = undefined as any

      await expect(api.revealInFileManager('test.md')).rejects.toThrow(
        'This backend does not support revealing items in the file manager'
      )
    })
  })

  describe('naming and URL validation', () => {
    it('should validate workspace name', async () => {
      vi.mocked(mockBackend.execute).mockResolvedValue({
        type: 'String',
        data: 'My Journal',
      })

      const result = await api.validateWorkspaceName('  My Journal  ', ['Other'], ['Remote'])

      expect(mockBackend.execute).toHaveBeenCalledWith({
        type: 'ValidateWorkspaceName',
        params: {
          name: '  My Journal  ',
          existing_local_names: ['Other'],
          existing_server_names: ['Remote'],
        },
      })
      expect(result).toBe('My Journal')
    })

    it('should validate workspace name without server names', async () => {
      vi.mocked(mockBackend.execute).mockResolvedValue({
        type: 'String',
        data: 'Journal',
      })

      await api.validateWorkspaceName('Journal', [])

      expect(mockBackend.execute).toHaveBeenCalledWith({
        type: 'ValidateWorkspaceName',
        params: {
          name: 'Journal',
          existing_local_names: [],
          existing_server_names: null,
        },
      })
    })

    it('should validate publishing slug', async () => {
      vi.mocked(mockBackend.execute).mockResolvedValue({ type: 'Ok' })

      await api.validatePublishingSlug('my-site')

      expect(mockBackend.execute).toHaveBeenCalledWith({
        type: 'ValidatePublishingSlug',
        params: { slug: 'my-site' },
      })
    })

    it('should normalize server URL', async () => {
      vi.mocked(mockBackend.execute).mockResolvedValue({
        type: 'String',
        data: 'https://example.com',
      })

      const result = await api.normalizeServerUrl('example.com')

      expect(mockBackend.execute).toHaveBeenCalledWith({
        type: 'NormalizeServerUrl',
        params: { url: 'example.com' },
      })
      expect(result).toBe('https://example.com')
    })
  })

  describe('plugin operations', () => {
    it('should get plugin manifests', async () => {
      const mockManifests = [{ id: 'diaryx.daily', name: 'Daily', version: '1.0.0', description: 'Daily entries', capabilities: [], ui: [], cli: [] }]
      vi.mocked(mockBackend.execute).mockResolvedValue({
        type: 'PluginManifests',
        data: mockManifests,
      })

      const result = await api.getPluginManifests()

      expect(mockBackend.execute).toHaveBeenCalledWith({
        type: 'GetPluginManifests',
      })
      expect(result).toEqual(mockManifests)
    })

    it('should get plugin config', async () => {
      const mockConfig = { theme: 'dark' }
      vi.mocked(mockBackend.execute).mockResolvedValue({
        type: 'PluginResult',
        data: mockConfig,
      })

      const result = await api.getPluginConfig('diaryx.daily')

      expect(mockBackend.execute).toHaveBeenCalledWith({
        type: 'GetPluginConfig',
        params: { plugin: 'diaryx.daily' },
      })
      expect(result).toEqual(mockConfig)
    })

    it('should set plugin config', async () => {
      vi.mocked(mockBackend.execute).mockResolvedValue({ type: 'Ok' })

      await api.setPluginConfig('diaryx.daily', { theme: 'light' })

      expect(mockBackend.execute).toHaveBeenCalledWith({
        type: 'SetPluginConfig',
        params: { plugin: 'diaryx.daily', config: { theme: 'light' } },
      })
    })

    it('should remove workspace plugin data', async () => {
      vi.mocked(mockBackend.execute).mockResolvedValue({ type: 'Ok' })

      await api.removeWorkspacePluginData('README.md', 'diaryx.daily')

      expect(mockBackend.execute).toHaveBeenCalledWith({
        type: 'RemoveWorkspacePluginData',
        params: { root_index_path: 'README.md', plugin: 'diaryx.daily' },
      })
    })
  })

  describe('permission denial handling', () => {
    it('should throw permission denied when user rejects the permission request', async () => {
      mockBackend.getWorkspacePath = vi.fn().mockReturnValue('/Users/test/journal/README.md')
      permissionStoreMocks.permissionStore.requestPermission.mockResolvedValue(false)

      vi.mocked(mockBackend.execute).mockRejectedValueOnce(
        new BackendError(
          "Permission not configured for plugin 'diaryx.daily': read_files on '/Users/test/journal/README.md'",
          'PluginError',
        ),
      )

      await expect(api.executePluginCommand('diaryx.daily', 'OpenToday', {})).rejects.toThrow(
        'permission_denied',
      )
      expect(permissionStoreMocks.permissionStore.requestPermission).toHaveBeenCalled()
      expect(mockBackend.execute).toHaveBeenCalledTimes(1)
    })

    it('should re-throw non-permission errors without prompting', async () => {
      vi.mocked(mockBackend.execute).mockRejectedValueOnce(
        new Error('Some random backend error'),
      )

      await expect(api.executePluginCommand('diaryx.daily', 'OpenToday', {})).rejects.toThrow(
        'Some random backend error',
      )
      expect(permissionStoreMocks.permissionStore.requestPermission).not.toHaveBeenCalled()
    })

    it('should re-throw "denied" (not "not_configured") permission errors without prompting', async () => {
      vi.mocked(mockBackend.execute).mockRejectedValueOnce(
        new BackendError(
          "Permission denied for plugin 'diaryx.daily': read_files on '/Users/test/journal/README.md'",
          'PluginError',
        ),
      )

      await expect(api.executePluginCommand('diaryx.daily', 'OpenToday', {})).rejects.toThrow(
        "Permission denied for plugin 'diaryx.daily'"
      )
      expect(permissionStoreMocks.permissionStore.requestPermission).not.toHaveBeenCalled()
    })
  })

  describe('getPluginComponentHtml edge cases', () => {
    it('should throw when plugin returns invalid component HTML via fallback path', async () => {
      vi.mocked(mockBackend.execute).mockResolvedValue({
        type: 'PluginResult',
        data: 12345,
      } as any)

      await expect(api.getPluginComponentHtml('diaryx.daily', 'daily.panel')).rejects.toThrow(
        'Plugin diaryx.daily returned invalid component HTML',
      )
    })

    it('should extract html from object with response key', async () => {
      vi.mocked(mockBackend.execute).mockResolvedValue({
        type: 'PluginResult',
        data: { response: '<div>Hello</div>' },
      } as any)

      const result = await api.getPluginComponentHtml('diaryx.daily', 'daily.panel')
      expect(result).toBe('<div>Hello</div>')
    })

    it('should extract html from object with html key', async () => {
      vi.mocked(mockBackend.execute).mockResolvedValue({
        type: 'PluginResult',
        data: { html: '<div>Hello</div>' },
      } as any)

      const result = await api.getPluginComponentHtml('diaryx.daily', 'daily.panel')
      expect(result).toBe('<div>Hello</div>')
    })

    it('should extract html from object with data key', async () => {
      vi.mocked(mockBackend.execute).mockResolvedValue({
        type: 'PluginResult',
        data: { data: '<div>Hello</div>' },
      } as any)

      const result = await api.getPluginComponentHtml('diaryx.daily', 'daily.panel')
      expect(result).toBe('<div>Hello</div>')
    })

    it('should extract html from nested PluginResult', async () => {
      vi.mocked(mockBackend.execute).mockResolvedValue({
        type: 'PluginResult',
        data: { type: 'PluginResult', data: '<div>Nested</div>' },
      } as any)

      const result = await api.getPluginComponentHtml('diaryx.daily', 'daily.panel')
      expect(result).toBe('<div>Nested</div>')
    })

    it('should extract html from success wrapper', async () => {
      vi.mocked(mockBackend.execute).mockResolvedValue({
        type: 'PluginResult',
        data: { success: true, data: '<div>Success</div>' },
      } as any)

      const result = await api.getPluginComponentHtml('diaryx.daily', 'daily.panel')
      expect(result).toBe('<div>Success</div>')
    })

    it('should return null-like for array data', async () => {
      vi.mocked(mockBackend.execute).mockResolvedValue({
        type: 'PluginResult',
        data: ['not', 'html'],
      } as any)

      await expect(api.getPluginComponentHtml('diaryx.daily', 'daily.panel')).rejects.toThrow(
        'Plugin diaryx.daily returned invalid component HTML',
      )
    })
  })

  describe('resolveWorkspaceRootIndexPath edge cases', () => {
    it('should return null when no candidates are valid', async () => {
      mockBackend.getWorkspacePath = vi.fn().mockReturnValue(null)

      const result = await api.resolveWorkspaceRootIndexPath(null)

      expect(result).toBeNull()
    })

    it('should deduplicate candidates', async () => {
      mockBackend.getWorkspacePath = vi.fn().mockReturnValue('/Users/test/journal/')
      vi.mocked(mockBackend.execute).mockResolvedValue({
        type: 'String',
        data: '/Users/test/journal/README.md',
      })

      const result = await api.resolveWorkspaceRootIndexPath('/Users/test/journal/')

      expect(result).toBe('/Users/test/journal/README.md')
      // Should only call FindRootIndex once because both candidates normalize to same value
      expect(mockBackend.execute).toHaveBeenCalledTimes(1)
    })

    it('should recognize index.md as a root index path', async () => {
      const result = await api.resolveWorkspaceRootIndexPath('/Users/test/journal/index.md')

      expect(mockBackend.execute).not.toHaveBeenCalled()
      expect(result).toBe('/Users/test/journal/index.md')
    })
  })

  describe('deleteEntry with hard_delete', () => {
    it('should delete entry with default hard_delete=false', async () => {
      vi.mocked(mockBackend.execute).mockResolvedValue({ type: 'Ok' })

      await api.deleteEntry('test.md')

      expect(mockBackend.execute).toHaveBeenCalledWith({
        type: 'DeleteEntry',
        params: { path: 'test.md', hard_delete: false },
      })
    })
  })

  describe('searchWorkspace with property filter', () => {
    it('should search with property option', async () => {
      vi.mocked(mockBackend.execute).mockResolvedValue({
        type: 'SearchResults',
        data: { files: [], files_searched: 3 },
      })

      await api.searchWorkspace('test', { property: 'title' })

      expect(mockBackend.execute).toHaveBeenCalledWith({
        type: 'SearchWorkspace',
        params: {
          pattern: 'test',
          options: {
            workspace_path: null,
            search_frontmatter: false,
            property: 'title',
            case_sensitive: false,
          },
        },
      })
    })
  })

  describe('getWorkspaceTree with audience', () => {
    it('should pass audience parameter', async () => {
      vi.mocked(mockBackend.execute).mockResolvedValue({
        type: 'Tree',
        data: { path: 'root', name: 'root', description: null, is_index: false, children: [], properties: {} },
      })

      await api.getWorkspaceTree('root', 2, 'public')

      expect(mockBackend.execute).toHaveBeenCalledWith({
        type: 'GetWorkspaceTree',
        params: { path: 'root', depth: 2, audience: 'public' },
      })
    })
  })
})
