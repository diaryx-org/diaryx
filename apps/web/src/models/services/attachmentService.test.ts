import { describe, it, expect, vi, beforeEach } from 'vitest'
import {
  getMimeType,
  isHeicFile,
  getAttachmentMediaKind,
  formatMarkdownDestination,
  isPreviewableAttachmentKind,
  convertHeicToJpeg,
  revokeBlobUrls,
  clearAttachmentThumbnailCache,
  clearAttachmentVerificationCache,
  transformAttachmentPaths,
  queueResolveAttachment,
  reverseBlobUrlsToAttachmentPaths,
  getBlobUrl,
  trackBlobUrl,
  hasBlobUrls,
  attachmentExistsLocally,
  getAttachmentAvailability,
  computeRelativeAttachmentPath,
} from './attachmentService'

describe('attachmentService', () => {
  beforeEach(() => {
    // Clean up blob URLs before each test
    revokeBlobUrls()
    clearAttachmentThumbnailCache()
    clearAttachmentVerificationCache()
  })

  describe('getMimeType', () => {
    it('should return correct MIME type for common image extensions', () => {
      expect(getMimeType('photo.png')).toBe('image/png')
      expect(getMimeType('photo.jpg')).toBe('image/jpeg')
      expect(getMimeType('photo.jpeg')).toBe('image/jpeg')
      expect(getMimeType('photo.gif')).toBe('image/gif')
      expect(getMimeType('photo.webp')).toBe('image/webp')
      expect(getMimeType('photo.svg')).toBe('image/svg+xml')
      expect(getMimeType('photo.bmp')).toBe('image/bmp')
      expect(getMimeType('favicon.ico')).toBe('image/x-icon')
    })

    it('should return correct MIME type for HEIC/HEIF', () => {
      expect(getMimeType('photo.heic')).toBe('image/heic')
      expect(getMimeType('photo.heif')).toBe('image/heif')
    })

    it('should return correct MIME type for document extensions', () => {
      expect(getMimeType('doc.pdf')).toBe('application/pdf')
      expect(getMimeType('doc.doc')).toBe('application/msword')
      expect(getMimeType('doc.docx')).toBe('application/vnd.openxmlformats-officedocument.wordprocessingml.document')
      expect(getMimeType('doc.xls')).toBe('application/vnd.ms-excel')
      expect(getMimeType('doc.xlsx')).toBe('application/vnd.openxmlformats-officedocument.spreadsheetml.sheet')
      expect(getMimeType('doc.ppt')).toBe('application/vnd.ms-powerpoint')
      expect(getMimeType('doc.pptx')).toBe('application/vnd.openxmlformats-officedocument.presentationml.presentation')
    })

    it('should return correct MIME type for text files', () => {
      expect(getMimeType('file.txt')).toBe('text/plain')
      expect(getMimeType('file.html')).toBe('text/html')
      expect(getMimeType('file.htm')).toBe('text/html')
      expect(getMimeType('file.md')).toBe('text/markdown')
      expect(getMimeType('file.csv')).toBe('text/csv')
      expect(getMimeType('file.json')).toBe('application/json')
      expect(getMimeType('file.xml')).toBe('application/xml')
    })

    it('should return correct MIME type for archives', () => {
      expect(getMimeType('archive.zip')).toBe('application/zip')
      expect(getMimeType('archive.tar')).toBe('application/x-tar')
      expect(getMimeType('archive.gz')).toBe('application/gzip')
      expect(getMimeType('archive.7z')).toBe('application/x-7z-compressed')
      expect(getMimeType('archive.rar')).toBe('application/vnd.rar')
    })

    it('should return octet-stream for unknown extensions', () => {
      expect(getMimeType('file.unknown')).toBe('application/octet-stream')
      expect(getMimeType('file.xyz')).toBe('application/octet-stream')
      expect(getMimeType('file')).toBe('application/octet-stream')
    })

    it('should be case-insensitive', () => {
      expect(getMimeType('photo.PNG')).toBe('image/png')
      expect(getMimeType('photo.JPG')).toBe('image/jpeg')
      expect(getMimeType('photo.HEIC')).toBe('image/heic')
    })

    it('should handle paths with directories', () => {
      expect(getMimeType('folder/subfolder/photo.png')).toBe('image/png')
      expect(getMimeType('/absolute/path/doc.pdf')).toBe('application/pdf')
    })
  })

  describe('isHeicFile', () => {
    it('should return true for HEIC files', () => {
      expect(isHeicFile('photo.heic')).toBe(true)
      expect(isHeicFile('photo.HEIC')).toBe(true)
    })

    it('should return true for HEIF files', () => {
      expect(isHeicFile('photo.heif')).toBe(true)
      expect(isHeicFile('photo.HEIF')).toBe(true)
    })

    it('should return false for other file types', () => {
      expect(isHeicFile('photo.jpg')).toBe(false)
      expect(isHeicFile('photo.png')).toBe(false)
      expect(isHeicFile('photo.gif')).toBe(false)
    })

    it('should handle paths with directories', () => {
      expect(isHeicFile('folder/photo.heic')).toBe(true)
      expect(isHeicFile('/path/to/photo.heif')).toBe(true)
    })
  })

  describe('getAttachmentMediaKind', () => {
    it('should classify image, video, audio, and generic files from extensions', () => {
      expect(getAttachmentMediaKind('photo.png')).toBe('image')
      expect(getAttachmentMediaKind('clip.mp4')).toBe('video')
      expect(getAttachmentMediaKind('voice.mp3')).toBe('audio')
      expect(getAttachmentMediaKind('doc.pdf')).toBe('file')
    })

    it('should prefer MIME type when extension is ambiguous', () => {
      expect(getAttachmentMediaKind('sample.ogg', 'audio/ogg')).toBe('audio')
      expect(getAttachmentMediaKind('sample.ogg', 'video/ogg')).toBe('video')
    })
  })

  describe('isPreviewableAttachmentKind', () => {
    it('should only allow inline preview for media kinds', () => {
      expect(isPreviewableAttachmentKind('image')).toBe(true)
      expect(isPreviewableAttachmentKind('video')).toBe(true)
      expect(isPreviewableAttachmentKind('audio')).toBe(true)
      expect(isPreviewableAttachmentKind('file')).toBe(false)
    })
  })

  describe('convertHeicToJpeg', () => {
    it('should convert HEIC blob to JPEG when converter plugin is registered', async () => {
      const { registerTranscoder, clearAllTranscoders } = await import('./imageConverterService')
      const mockPlugin = {
        callBinary: vi.fn().mockResolvedValue((() => {
          // Build a valid wire-format response: status=0 (ok), format=0 (jpeg), reserved=0,0, payload_len, payload
          const jpegPayload = new TextEncoder().encode('mock-jpeg-output')
          const buf = new Uint8Array(8 + jpegPayload.byteLength)
          const view = new DataView(buf.buffer)
          view.setUint8(0, 0) // status ok
          view.setUint8(1, 0) // format jpeg
          view.setUint16(2, 0) // reserved
          view.setUint32(4, jpegPayload.byteLength, true)
          buf.set(jpegPayload, 8)
          return buf
        })()),
      }
      registerTranscoder('test-heic-plugin', mockPlugin as any, ['heic:jpeg'])

      const heicBlob = new Blob(['mock-heic-data'], { type: 'image/heic' })
      const result = await convertHeicToJpeg(heicBlob)

      expect(result).toBeInstanceOf(Blob)
      expect(result.type).toBe('image/jpeg')
      clearAllTranscoders()
    })

    it('should return original blob when no converter plugin is registered', async () => {
      const { clearAllTranscoders } = await import('./imageConverterService')
      clearAllTranscoders()

      const heicBlob = new Blob(['mock-heic-data'], { type: 'image/heic' })
      const result = await convertHeicToJpeg(heicBlob)

      expect(result).toBe(heicBlob)
    })
  })

  describe('blob URL tracking', () => {
    it('should track blob URLs', () => {
      expect(hasBlobUrls()).toBe(false)

      trackBlobUrl('image.png', 'blob:mock-url-1')
      expect(hasBlobUrls()).toBe(true)
      expect(getBlobUrl('image.png')).toBe('blob:mock-url-1')
    })

    it('should return undefined for untracked paths', () => {
      expect(getBlobUrl('unknown.png')).toBeUndefined()
    })

    it('should revoke all blob URLs', () => {
      trackBlobUrl('image1.png', 'blob:url-1')
      trackBlobUrl('image2.png', 'blob:url-2')

      expect(hasBlobUrls()).toBe(true)

      revokeBlobUrls()

      expect(hasBlobUrls()).toBe(false)
      expect(getBlobUrl('image1.png')).toBeUndefined()
      expect(getBlobUrl('image2.png')).toBeUndefined()
    })
  })

  describe('attachment availability', () => {
    it('should check local existence without reading attachment bytes', async () => {
      const mockApi = {
        resolveAttachmentStoragePath: vi.fn().mockResolvedValue('/workspace/_attachments/test.png'),
        fileExists: vi.fn().mockResolvedValue(true),
      }

      const result = await attachmentExistsLocally(
        mockApi as any,
        'entry.md',
        '_attachments/test.png',
      )

      expect(result).toBe(true)
      expect(mockApi.resolveAttachmentStoragePath).toHaveBeenCalledWith('entry.md', '_attachments/test.png')
      expect(mockApi.fileExists).toHaveBeenCalledWith('/workspace/_attachments/test.png')
    })

    it('should report remote availability when local file is missing but metadata exists', async () => {
      const mockApi = {
        resolveAttachmentStoragePath: vi.fn().mockResolvedValue('/workspace/_attachments/test.png'),
        fileExists: vi.fn().mockResolvedValue(false),
      }

      const { indexAttachmentRefs } = await import('$lib/sync/attachmentSyncService')
      indexAttachmentRefs(
        'entry-remote.md',
        [{
          path: '_attachments/test.png',
          source: 'local',
          hash: 'abc123',
          mime_type: 'image/png',
          size: BigInt(12),
          uploaded_at: BigInt(Date.now()),
          deleted: false,
        }],
        'ws-1',
      )

      const result = await getAttachmentAvailability(
        mockApi as any,
        'entry-remote.md',
        '_attachments/test.png',
      )

      expect(result).toBe('remote')
    })
  })

  describe('transformAttachmentPaths', () => {
    it('should return original content if api is null', async () => {
      const content = '![alt](image.png)'
      const result = await transformAttachmentPaths(content, 'entry.md', null)
      expect(result).toBe(content)
    })

    it('should skip external URLs', async () => {
      const mockApi = {
        resolveAttachmentStoragePath: vi.fn(),
        readBinary: vi.fn(),
        canonicalizeLink: vi.fn(),
        formatLink: vi.fn(),
      }

      const content = '![alt](https://example.com/image.png)'
      const result = await transformAttachmentPaths(content, 'entry.md', mockApi as any)

      expect(result).toBe(content)
      expect(mockApi.resolveAttachmentStoragePath).not.toHaveBeenCalled()
    })

    it('should skip already-transformed blob URLs', async () => {
      const mockApi = {
        resolveAttachmentStoragePath: vi.fn(),
        readBinary: vi.fn(),
        canonicalizeLink: vi.fn(),
        formatLink: vi.fn(),
      }

      const content = '![alt](blob:http://localhost/123)'
      const result = await transformAttachmentPaths(content, 'entry.md', mockApi as any)

      expect(result).toBe(content)
      expect(mockApi.resolveAttachmentStoragePath).not.toHaveBeenCalled()
    })

    it('should transform local image paths to blob URLs', async () => {
      const mockData = new Uint8Array([1, 2, 3])
      const mockApi = {
        resolveAttachmentStoragePath: vi.fn().mockResolvedValue('resolved/test.png'),
        readBinary: vi.fn().mockResolvedValue(mockData),
        canonicalizeLink: vi.fn(async (path: string) => path),
        formatLink: vi.fn(async (path: string) => path),
      }

      const content = '![test image](test.png)'
      const result = await transformAttachmentPaths(content, 'entry.md', mockApi as any)

      expect(mockApi.resolveAttachmentStoragePath).toHaveBeenCalledWith('entry.md', 'test.png')
      expect(mockApi.readBinary).toHaveBeenCalledWith('resolved/test.png')
      expect(mockApi.canonicalizeLink).not.toHaveBeenCalled()
      expect(mockApi.formatLink).not.toHaveBeenCalled()
      expect(result).toContain('blob:')
      expect(result).toContain('![test image]')
    })

    it('should cache successful attachment hash verification across blob URL resets', async () => {
      const expectedHash = 'a'.repeat(64)
      const syncService = await import('$lib/sync/attachmentSyncService')
      const shaSpy = vi.spyOn(syncService, 'sha256Hex').mockResolvedValue(expectedHash)

      syncService.indexAttachmentRefs(
        'entry.md',
        [{
          path: 'test.png',
          source: 'local',
          hash: expectedHash,
          mime_type: 'image/png',
          size: BigInt(3),
          uploaded_at: BigInt(Date.now()),
          deleted: false,
        }],
        'ws-1',
      )

      const mockData = new Uint8Array([1, 2, 3])
      const mockApi = {
        resolveAttachmentStoragePath: vi.fn().mockResolvedValue('resolved/test.png'),
        readBinary: vi.fn().mockResolvedValue(mockData),
        canonicalizeLink: vi.fn(async (path: string) => path),
        formatLink: vi.fn(async (path: string) => path),
      }

      await transformAttachmentPaths('![alt](test.png)', 'entry.md', mockApi as any)
      revokeBlobUrls()
      await transformAttachmentPaths('![alt](test.png)', 'entry.md', mockApi as any)

      expect(mockApi.readBinary).toHaveBeenCalledTimes(2)
      expect(shaSpy).toHaveBeenCalledTimes(1)
      shaSpy.mockRestore()
    })

    it('should fall back to link-parser normalization when direct lookup fails', async () => {
      const mockData = new Uint8Array([1, 2, 3])
      const mockApi = {
        resolveAttachmentStoragePath: vi.fn(async (_entryPath: string, attachmentPath: string) => {
          if (attachmentPath === 'raw/path.png') {
            throw new Error('Not found')
          }
          if (attachmentPath === 'normalized/path.png') {
            return 'resolved/normalized/path.png'
          }
          throw new Error('Unexpected path')
        }),
        readBinary: vi.fn().mockResolvedValue(mockData),
        canonicalizeLink: vi.fn(async () => 'normalized/path.png'),
        formatLink: vi.fn(async () => 'normalized/path.png'),
      }

      const content = '![alt](raw/path.png)'
      const result = await transformAttachmentPaths(content, 'entry.md', mockApi as any)

      expect(mockApi.resolveAttachmentStoragePath).toHaveBeenNthCalledWith(1, 'entry.md', 'raw/path.png')
      expect(mockApi.canonicalizeLink).toHaveBeenCalledWith('raw/path.png', 'entry.md')
      expect(mockApi.formatLink).toHaveBeenCalledWith(
        'normalized/path.png',
        'path.png',
        'plain_relative',
        'entry.md',
      )
      expect(mockApi.resolveAttachmentStoragePath).toHaveBeenNthCalledWith(2, 'entry.md', 'normalized/path.png')
      expect(result).toContain('blob:')
    })

    it('should handle angle-bracket syntax for paths with spaces', async () => {
      const mockData = new Uint8Array([1, 2, 3])
      const mockApi = {
        resolveAttachmentStoragePath: vi.fn().mockResolvedValue('resolved/path with spaces/image.png'),
        readBinary: vi.fn().mockResolvedValue(mockData),
        canonicalizeLink: vi.fn(async (path: string) => path),
        formatLink: vi.fn(async (path: string) => path),
      }

      const content = '![alt](<path with spaces/image.png>)'
      await transformAttachmentPaths(content, 'entry.md', mockApi as any)

      expect(mockApi.resolveAttachmentStoragePath).toHaveBeenCalledWith('entry.md', 'path with spaces/image.png')
    })

    it('should leave original path on attachment not found', async () => {
      const mockApi = {
        resolveAttachmentStoragePath: vi.fn().mockRejectedValue(new Error('Not found')),
        readBinary: vi.fn(),
        canonicalizeLink: vi.fn(async (path: string) => path),
        formatLink: vi.fn(async (path: string) => path),
      }

      const content = '![alt](missing.png)'
      const result = await transformAttachmentPaths(content, 'entry.md', mockApi as any)

      expect(result).toBe(content)
    })

    it('should normalize nested markdown-link paths via link_parser helpers', async () => {
      const mockData = new Uint8Array([1, 2, 3])
      const mockApi = {
        resolveAttachmentStoragePath: vi.fn().mockResolvedValue('resolved/_attachments/diaryx-icon.jpg'),
        readBinary: vi.fn().mockResolvedValue(mockData),
        canonicalizeLink: vi.fn(async (path: string) => {
          if (path.endsWith(')')) {
            return '_attachments/diaryx-icon.jpg'
          }
          throw new Error('unbalanced markdown link')
        }),
        formatLink: vi.fn(async () => '_attachments/diaryx-icon.jpg'),
      }

      const content = '![preview]([diaryx-icon.jpg](/_attachments/diaryx-icon.jpg))'
      const result = await transformAttachmentPaths(content, 'entry.md', mockApi as any)

      expect(mockApi.canonicalizeLink).toHaveBeenNthCalledWith(
        1,
        '[diaryx-icon.jpg](/_attachments/diaryx-icon.jpg',
        'entry.md'
      )
      expect(mockApi.canonicalizeLink).toHaveBeenNthCalledWith(
        2,
        '[diaryx-icon.jpg](/_attachments/diaryx-icon.jpg)',
        'entry.md'
      )
      expect(mockApi.resolveAttachmentStoragePath).toHaveBeenCalledWith('entry.md', '_attachments/diaryx-icon.jpg')
      expect(result).toContain('blob:')
    })

    it('should use the underlying asset MIME type for note-backed HTML refs', async () => {
      const mockData = new Uint8Array([60, 104, 49, 62])
      const createObjectUrlSpy = vi.spyOn(URL, 'createObjectURL')
      const mockApi = {
        resolveAttachmentStoragePath: vi.fn().mockResolvedValue('resolved/_attachments/sample.html'),
        readBinary: vi.fn().mockResolvedValue(mockData),
        canonicalizeLink: vi.fn(async (path: string) => path),
        formatLink: vi.fn(async (path: string) => path),
      }

      const content = '![Sample.html](_attachments/sample.html.md)'
      await transformAttachmentPaths(content, 'entry.md', mockApi as any)

      expect(createObjectUrlSpy).toHaveBeenCalled()
      const blobArg = createObjectUrlSpy.mock.calls.at(-1)?.[0]
      expect(blobArg).toBeInstanceOf(Blob)
      expect((blobArg as Blob).type).toBe('text/html')
      createObjectUrlSpy.mockRestore()
    })

    it('should keep direct HTML attachment refs previewable as text/html', async () => {
      const mockData = new Uint8Array([60, 104, 49, 62])
      const createObjectUrlSpy = vi.spyOn(URL, 'createObjectURL')
      const mockApi = {
        resolveAttachmentStoragePath: vi.fn().mockResolvedValue('resolved/_attachments/sample.html'),
        readBinary: vi.fn().mockResolvedValue(mockData),
        canonicalizeLink: vi.fn(async (path: string) => path),
        formatLink: vi.fn(async (path: string) => path),
      }

      const content = '![sample.html](_attachments/sample.html)'
      await transformAttachmentPaths(content, 'entry.md', mockApi as any)

      expect(createObjectUrlSpy).toHaveBeenCalled()
      const blobArg = createObjectUrlSpy.mock.calls.at(-1)?.[0]
      expect(blobArg).toBeInstanceOf(Blob)
      expect((blobArg as Blob).type).toBe('text/html')
      createObjectUrlSpy.mockRestore()
    })
  })

  describe('queueResolveAttachment', () => {
    it('should drain queued attachment resolutions after earlier images finish', async () => {
      let activeReads = 0
      let maxReads = 0
      const mockApi = {
        resolveAttachmentStoragePath: vi.fn(async (_entryPath: string, attachmentPath: string) => attachmentPath),
        readBinary: vi.fn(async (resolvedPath: string) => {
          activeReads += 1
          maxReads = Math.max(maxReads, activeReads)
          await Promise.resolve()
          activeReads -= 1
          return new TextEncoder().encode(resolvedPath)
        }),
      }

      const results = await Promise.all([
        queueResolveAttachment(mockApi as any, 'entry.md', '/img-1.png'),
        queueResolveAttachment(mockApi as any, 'entry.md', '/img-2.png'),
        queueResolveAttachment(mockApi as any, 'entry.md', '/img-3.png'),
      ])

      expect(results).toEqual(['blob:mock-url', 'blob:mock-url', 'blob:mock-url'])
      expect(mockApi.resolveAttachmentStoragePath).toHaveBeenCalledTimes(3)
      expect(mockApi.readBinary).toHaveBeenCalledTimes(3)
      expect(maxReads).toBe(1)
    })
  })

  describe('reverseBlobUrlsToAttachmentPaths', () => {
    it('should reverse blob URLs to original paths', () => {
      trackBlobUrl('image.png', 'blob:url-1')

      const content = 'Here is an image: ![alt](blob:url-1)'
      const result = reverseBlobUrlsToAttachmentPaths(content)

      expect(result).toBe('Here is an image: ![alt](image.png)')
    })

    it('should wrap paths with spaces in angle brackets', () => {
      trackBlobUrl('path with spaces/image.png', 'blob:url-2')

      const content = '![alt](blob:url-2)'
      const result = reverseBlobUrlsToAttachmentPaths(content)

      expect(result).toBe('![alt](<path with spaces/image.png>)')
    })

    it('should handle multiple blob URLs', () => {
      trackBlobUrl('image1.png', 'blob:url-1')
      trackBlobUrl('image2.png', 'blob:url-2')

      const content = '![one](blob:url-1) and ![two](blob:url-2)'
      const result = reverseBlobUrlsToAttachmentPaths(content)

      expect(result).toBe('![one](image1.png) and ![two](image2.png)')
    })

    it('should return original content if no blob URLs are tracked', () => {
      const content = '![alt](image.png)'
      const result = reverseBlobUrlsToAttachmentPaths(content)
      expect(result).toBe(content)
    })
  })

  describe('formatMarkdownDestination', () => {
    it('should wrap destinations with whitespace in angle brackets', () => {
      expect(formatMarkdownDestination('path with spaces/video.mov')).toBe(
        '<path with spaces/video.mov>',
      )
    })

    it('should preserve already wrapped destinations', () => {
      expect(formatMarkdownDestination('<path with spaces/video.mov>')).toBe(
        '<path with spaces/video.mov>',
      )
    })

    it('should leave plain destinations unchanged', () => {
      expect(formatMarkdownDestination('_attachments/video.mov')).toBe(
        '_attachments/video.mov',
      )
    })
  })

  describe('computeRelativeAttachmentPath', () => {
    it('should return attachment path if same entry', () => {
      const result = computeRelativeAttachmentPath(
        '2025/01/day.md',
        '2025/01/day.md',
        'image.png'
      )
      expect(result).toBe('image.png')
    })

    it('should compute relative path from child to parent', () => {
      const result = computeRelativeAttachmentPath(
        '2025/01/day.md',
        '2025/01.index.md',
        'header.png'
      )
      expect(result).toBe('../header.png')
    })

    it('should compute relative path between sibling directories', () => {
      const result = computeRelativeAttachmentPath(
        '2025/02/day.md',
        '2025/01/note.md',
        'image.png'
      )
      expect(result).toBe('../01/image.png')
    })

    it('should compute relative path from deeply nested to root', () => {
      const result = computeRelativeAttachmentPath(
        '2025/01/15/entry.md',
        '2025.index.md',
        'banner.png'
      )
      // 2025/01/15/entry.md -> 2025.index.md needs 3 levels up: ../../../
      expect(result).toBe('../../../banner.png')
    })

    it('should handle root-level entries', () => {
      const result = computeRelativeAttachmentPath(
        'notes.md',
        'index.md',
        'logo.png'
      )
      expect(result).toBe('logo.png')
    })

    it('should handle same directory different files', () => {
      const result = computeRelativeAttachmentPath(
        'docs/a.md',
        'docs/b.md',
        'shared.png'
      )
      expect(result).toBe('shared.png')
    })
  })
})
