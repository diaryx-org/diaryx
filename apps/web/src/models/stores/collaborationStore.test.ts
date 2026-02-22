import { describe, it, expect, beforeEach } from 'vitest'
import { getCollaborationStore } from './collaborationStore.svelte'

describe('collaborationStore', () => {
  let store: ReturnType<typeof getCollaborationStore>

  beforeEach(() => {
    store = getCollaborationStore()
    // Reset state
    store.clearCollaborationSession()
    store.setEnabled(false)
    store.setConnected(false)
    store.setServerUrl(null)
  })



  describe('collaboration path', () => {
    it('should initialize with null path', () => {
      expect(store.currentCollaborationPath).toBeNull()
    })

    it('should set collaboration path', () => {
      store.setCollaborationPath('workspace/entry.md')
      expect(store.currentCollaborationPath).toBe('workspace/entry.md')
    })

    it('should clear collaboration path', () => {
      store.setCollaborationPath('workspace/entry.md')
      store.setCollaborationPath(null)
      expect(store.currentCollaborationPath).toBeNull()
    })
  })

  describe('collaboration session', () => {
    it('should set collaboration session with path', () => {
      const path = 'workspace/entry.md'

      store.setCollaborationSession(path)

      expect(store.currentCollaborationPath).toBe(path)
    })

    it('should clear collaboration session', () => {
      store.setCollaborationSession('path')
      store.clearCollaborationSession()

      expect(store.currentCollaborationPath).toBeNull()
    })
  })

  describe('connection status', () => {
    it('should initialize with disabled collaboration', () => {
      expect(store.collaborationEnabled).toBe(false)
    })

    it('should set enabled state', () => {
      store.setEnabled(true)
      expect(store.collaborationEnabled).toBe(true)

      store.setEnabled(false)
      expect(store.collaborationEnabled).toBe(false)
    })

    it('should initialize as disconnected', () => {
      expect(store.collaborationConnected).toBe(false)
    })

    it('should set connected state', () => {
      store.setConnected(true)
      expect(store.collaborationConnected).toBe(true)

      store.setConnected(false)
      expect(store.collaborationConnected).toBe(false)
    })
  })

  describe('server URL', () => {
    it('should initialize with null server URL', () => {
      expect(store.collaborationServerUrl).toBeNull()
    })

    it('should set server URL', () => {
      store.setServerUrl('wss://sync.example.com')
      expect(store.collaborationServerUrl).toBe('wss://sync.example.com')
    })

    it('should not persist server URL to localStorage (managed by authStore)', () => {
      store.setServerUrl('wss://sync.example.com')
      expect(localStorage.setItem).not.toHaveBeenCalled()
    })
  })
})
