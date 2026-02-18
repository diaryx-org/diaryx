import { test as base, expect, Page, Locator } from '@playwright/test'

/**
 * Cross-platform modifier key.
 * Uses Meta (Cmd) on macOS, Control on Windows/Linux.
 */
export function getModifierKey(page: Page): string {
  // Playwright provides platform info via context
  const isMac = process.platform === 'darwin'
  return isMac ? 'Meta' : 'Control'
}

/**
 * Helper class for common editor interactions with proper waits.
 */
export class EditorHelper {
  readonly page: Page
  readonly editor: Locator

  constructor(page: Page) {
    this.page = page
    this.editor = page.locator('.ProseMirror, [contenteditable="true"]').first()
  }

  async waitForReady(): Promise<void> {
    await expect(this.editor).toBeVisible({ timeout: 15000 })
    // Wait for editor to be interactive (not just visible)
    await this.editor.waitFor({ state: 'attached' })
  }

  async focus(): Promise<void> {
    await this.editor.click()
    // Small wait for focus to settle
    await this.page.waitForFunction(() => {
      const el = document.querySelector('.ProseMirror, [contenteditable="true"]')
      return el && document.activeElement === el
    }, { timeout: 5000 }).catch(() => {
      // Fallback: focus was likely already achieved
    })
  }

  async type(text: string): Promise<void> {
    await this.page.keyboard.type(text)
  }

  async selectAll(): Promise<void> {
    const mod = getModifierKey(this.page)
    await this.page.keyboard.press(`${mod}+a`)
  }

  async clearContent(): Promise<void> {
    await this.selectAll()
    await this.page.keyboard.press('Backspace')
  }

  async applyBold(): Promise<void> {
    const mod = getModifierKey(this.page)
    await this.page.keyboard.press(`${mod}+b`)
  }

  async applyItalic(): Promise<void> {
    const mod = getModifierKey(this.page)
    await this.page.keyboard.press(`${mod}+i`)
  }

  async undo(): Promise<void> {
    // Use programmatic undo which is more reliable than keyboard shortcuts in tests
    await this.editor.evaluate(() => {
      const win = window as unknown as { tiptapEditor?: { commands?: { undo: () => void } } }
      if (win.tiptapEditor?.commands?.undo) {
        win.tiptapEditor.commands.undo()
      } else {
        document.execCommand('undo')
      }
    })
  }

  async redo(): Promise<void> {
    // Use programmatic redo which is more reliable than keyboard shortcuts in tests
    await this.editor.evaluate(() => {
      const win = window as unknown as { tiptapEditor?: { commands?: { redo: () => void } } }
      if (win.tiptapEditor?.commands?.redo) {
        win.tiptapEditor.commands.redo()
      } else {
        document.execCommand('redo')
      }
    })
  }

  /**
   * Opens the floating menu by clearing content on the current line.
   * Returns the plus button locator.
   */
  async openFloatingMenu(): Promise<Locator> {
    await this.focus()
    await this.type('temp')
    await this.clearContent()

    const plusButton = this.page.locator('.floating-menu .trigger-button')
    await expect(plusButton).toBeVisible({ timeout: 5000 })
    return plusButton
  }

  /**
   * Expands the floating menu by clicking the plus button.
   * This inserts a BlockPickerNode inline in the editor.
   */
  async expandFloatingMenu(): Promise<void> {
    const plusButton = await this.openFloatingMenu()
    await plusButton.click()
    await expect(this.page.locator('.block-picker-menu')).toBeVisible()
  }
}

/**
 * Complete the welcome screen onboarding flow if it appears.
 * Creates a default workspace so tests can proceed to the editor.
 */
async function handleWelcomeScreenIfNeeded(page: Page, timeoutMs: number): Promise<void> {
  const editor = page.locator('.ProseMirror, [contenteditable="true"]')
  const welcomeHeading = page.getByRole('heading', { name: 'Welcome to Diaryx' })

  // Wait for either the editor or welcome screen to appear
  await Promise.race([
    editor.first().waitFor({ state: 'visible', timeout: timeoutMs }),
    welcomeHeading.waitFor({ state: 'visible', timeout: timeoutMs }),
  ])

  // If the welcome screen appeared, complete it
  if (await welcomeHeading.isVisible().catch(() => false)) {
    const createButton = page.getByRole('button', { name: 'Create Workspace' })
    await expect(createButton).toBeVisible({ timeout: 5000 })
    await createButton.click()

    // Wait for the editor to load after workspace creation
    await editor.first().waitFor({ state: 'visible', timeout: timeoutMs })
  }
}

/**
 * Wait for the app to be fully initialized.
 * Handles the welcome screen onboarding if no workspaces exist.
 */
export async function waitForAppReady(page: Page, timeoutMs: number = 20000): Promise<void> {
  // Wait for the main app container
  await page.waitForSelector('body', { state: 'visible' })

  // Handle welcome screen if it appears (first run / cleared storage)
  await handleWelcomeScreenIfNeeded(page, timeoutMs)

  // Additional check: ensure no loading spinners are visible
  const spinner = page.locator('.loading, [data-loading="true"]')
  const spinnerTimeout = Math.min(10000, timeoutMs)
  await spinner.waitFor({ state: 'hidden', timeout: spinnerTimeout }).catch(() => {
    // No spinner found, which is fine
  })
}

/**
 * Clear browser storage between tests for isolation.
 */
export async function clearStorage(page: Page): Promise<void> {
  await page.evaluate(async () => {
    // Clear localStorage
    localStorage.clear()

    // Clear sessionStorage
    sessionStorage.clear()

    // Clear IndexedDB databases
    const databases = await indexedDB.databases?.() ?? []
    for (const db of databases) {
      if (db.name) {
        indexedDB.deleteDatabase(db.name)
      }
    }
  })
}

/**
 * Clear ALL browser storage including OPFS for complete test isolation.
 * This ensures each client starts with a fresh, minimal 1-file workspace.
 */
export async function clearAllBrowserStorage(page: Page): Promise<void> {
  await page.evaluate(async () => {
    // Clear localStorage and sessionStorage
    localStorage.clear()
    sessionStorage.clear()

    // Clear all IndexedDB databases
    const dbs = await indexedDB.databases?.() ?? []
    for (const db of dbs) {
      if (db.name) {
        indexedDB.deleteDatabase(db.name)
      }
    }

    // Clear OPFS (Origin Private File System) where SQLite CRDT db lives
    try {
      const root = await navigator.storage.getDirectory()
      for await (const [name] of root.entries()) {
        await root.removeEntry(name, { recursive: true })
      }
    } catch {
      // OPFS may not be supported in all browsers
    }
  })
}

/**
 * Mock WebSocket for share/collaboration tests.
 */
export async function mockWebSocket(page: Page): Promise<void> {
  await page.addInitScript(() => {
    // Store original WebSocket
    const OriginalWebSocket = window.WebSocket

    // Create mock WebSocket class
    class MockWebSocket {
      static CONNECTING = 0
      static OPEN = 1
      static CLOSING = 2
      static CLOSED = 3

      url: string
      readyState: number = MockWebSocket.CONNECTING
      onopen: ((event: Event) => void) | null = null
      onclose: ((event: CloseEvent) => void) | null = null
      onmessage: ((event: MessageEvent) => void) | null = null
      onerror: ((event: Event) => void) | null = null

      constructor(url: string) {
        this.url = url
        // Simulate connection opening after a brief delay
        setTimeout(() => {
          this.readyState = MockWebSocket.OPEN
          if (this.onopen) {
            this.onopen(new Event('open'))
          }
        }, 100)
      }

      send(_data: unknown): void {
        // Mock: do nothing
      }

      close(): void {
        this.readyState = MockWebSocket.CLOSED
        if (this.onclose) {
          this.onclose(new CloseEvent('close'))
        }
      }

      addEventListener(type: string, listener: EventListener): void {
        if (type === 'open') this.onopen = listener as (event: Event) => void
        if (type === 'close') this.onclose = listener as (event: CloseEvent) => void
        if (type === 'message') this.onmessage = listener as (event: MessageEvent) => void
        if (type === 'error') this.onerror = listener as (event: Event) => void
      }

      removeEventListener(): void {
        // Mock: do nothing
      }
    }

    // Replace WebSocket globally
    (window as unknown as { WebSocket: typeof MockWebSocket }).WebSocket = MockWebSocket as unknown as typeof WebSocket

    // Store original for potential restoration
    (window as unknown as { __OriginalWebSocket: typeof WebSocket }).__OriginalWebSocket = OriginalWebSocket
  })
}

/**
 * Mock authentication state for tests that require a signed-in user.
 * Sets localStorage items so initAuth() treats the user as authenticated,
 * and mocks the /api/me endpoint that initAuth() calls to validate the token.
 */
export async function mockAuth(page: Page): Promise<void> {
  await page.addInitScript(() => {
    localStorage.setItem('diaryx_auth_token', 'test-token-e2e')
    localStorage.setItem('diaryx_sync_server_url', 'http://localhost:9999')
    localStorage.setItem('diaryx_user', JSON.stringify({
      id: 'test-user-id',
      email: 'test@example.com',
    }))
  })

  // Mock the /auth/me validation call that initAuth() makes on startup.
  // Return empty workspaces to prevent SyncSetupWizard from auto-opening,
  // and tier: "plus" so the Host button is visible (not gated behind upgrade).
  await page.route('**/auth/me', route => {
    route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({
        user: { id: 'test-user-id', email: 'test@example.com' },
        workspaces: [],
        devices: [],
        tier: 'plus',
        workspace_limit: 10,
        published_site_limit: 10,
        attachment_limit_bytes: 200 * 1024 * 1024,
      }),
    })
  })
}

// Extended test fixtures
interface TestFixtures {
  editorHelper: EditorHelper
  modKey: string
}

export const test = base.extend<TestFixtures>({
  editorHelper: async ({ page }, use) => {
    const helper = new EditorHelper(page)
    await use(helper)
  },
  modKey: async ({ page }, use) => {
    await use(getModifierKey(page))
  },
})

export { expect }
