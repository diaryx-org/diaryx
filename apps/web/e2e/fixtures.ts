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
    // Wait for the TipTap editor instance to be fully initialized
    await this.page.waitForFunction(() => {
      const editor = (globalThis as any).__diaryx_tiptapEditor
      return editor && !editor.isDestroyed
    }, { timeout: 10000 })
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
    // Wait for the editor to settle into a single empty paragraph
    await this.page.waitForFunction(() => {
      const el = document.querySelector('.ProseMirror, [contenteditable="true"]')
      if (!el) return false
      const text = (el.textContent || '').trim()
      return text.length === 0
    }, { timeout: 3000 }).catch(() => {
      // Content may not fully clear on first try - do another pass
    })
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
    const mod = getModifierKey(this.page)
    await this.page.keyboard.press(`${mod}+z`)
  }

  async redo(): Promise<void> {
    const mod = getModifierKey(this.page)
    const isMac = process.platform === 'darwin'
    if (isMac) {
      await this.page.keyboard.press(`${mod}+Shift+z`)
    } else {
      await this.page.keyboard.press(`${mod}+y`)
    }
  }

  /**
   * Opens the floating menu by clearing content on the current line.
   * Returns the plus button locator.
   */
  async openFloatingMenu(): Promise<Locator> {
    const plusButton = this.page.locator('.floating-menu .trigger-button')

    // Retry the clear-and-wait cycle if the floating menu doesn't appear
    for (let attempt = 0; attempt < 2; attempt++) {
      await this.focus()
      await this.type('temp')
      await this.clearContent()

      try {
        await expect(plusButton).toBeVisible({ timeout: 5000 })
        return plusButton
      } catch {
        if (attempt === 1) throw new Error('Floating menu did not appear after clearing content')
      }
    }

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
async function completeAddWorkspaceDialog(page: Page, timeoutMs: number): Promise<void> {
  const addWorkspaceDialog = page.getByRole('dialog', { name: 'Add Workspace' })
  if (!(await addWorkspaceDialog.isVisible().catch(() => false))) {
    return
  }

  const startFreshButton = addWorkspaceDialog.getByRole('button', { name: /start fresh/i }).first()
  if (await startFreshButton.isVisible().catch(() => false)) {
    await startFreshButton.click().catch(() => undefined)
  }

  const emptyWorkspaceButton = addWorkspaceDialog.getByRole('button', { name: /empty workspace/i }).first()
  if (await emptyWorkspaceButton.isVisible().catch(() => false)) {
    await emptyWorkspaceButton.click().catch(() => undefined)
  }

  const createWorkspaceButton = addWorkspaceDialog.getByRole('button', {
    name: /create workspace|create & sync|import workspace|open workspace|download workspace/i,
  })
  await expect(createWorkspaceButton).toBeVisible({ timeout: 5000 })
  await expect(createWorkspaceButton).toBeEnabled({ timeout: timeoutMs })
  await createWorkspaceButton.click()

  const dialogHideTimeout = Math.min(timeoutMs, 15000)
  const dialogClosed = await addWorkspaceDialog
    .waitFor({ state: 'hidden', timeout: dialogHideTimeout })
    .then(() => true)
    .catch(() => false)

  if (!dialogClosed) {
    throw new Error('Add Workspace dialog did not close after submission')
  }
}

async function bootstrapWorkspaceFallback(page: Page): Promise<void> {
  await page.evaluate(async () => {
    const registry = await import('/src/lib/storage/localWorkspaceRegistry.svelte')
    const backendModule = await import('/src/lib/backend')

    const currentWorkspaceId = registry.getCurrentWorkspaceId()
    const existingWorkspace = currentWorkspaceId
      ? registry.getLocalWorkspace(currentWorkspaceId)
      : null
    const workspace = existingWorkspace ?? registry.getLocalWorkspaces()[0] ?? registry.createLocalWorkspace('My Workspace')

    registry.setCurrentWorkspaceId(workspace.id)
    backendModule.resetBackend()

    const backend = await backendModule.getBackend(
      workspace.id,
      workspace.name,
      registry.getWorkspaceStorageType(workspace.id),
      registry.getWorkspaceStoragePluginId(workspace.id),
    )
    const api = backendModule.createApi(backend)
    const workspaceDir = backend.getWorkspacePath()
      .replace(/\/index\.md$/, '')
      .replace(/\/README\.md$/, '')

    const isAlreadyExistsError = (error: unknown): boolean => {
      const message = error instanceof Error ? error.message : String(error)
      return (
        message.includes('Workspace already exists') ||
        message.includes('WorkspaceAlreadyExists')
      )
    }

    try {
      await api.findRootIndex(workspaceDir)
    } catch {
      try {
        await api.createWorkspace(workspaceDir, workspace.name)
      } catch (error) {
        if (!isAlreadyExistsError(error)) {
          throw error
        }
      }
    }
  })

  await page.reload({ waitUntil: 'domcontentloaded' })
}

async function handleWelcomeScreenIfNeeded(page: Page, timeoutMs: number): Promise<void> {
  const editor = page.locator('.ProseMirror, [contenteditable="true"]')
  const appSurface = page.getByRole('application').first()
  const welcomeHeading = page.getByRole('heading', { name: 'Welcome to Diaryx' })
  const addWorkspaceDialog = page.getByRole('dialog', { name: 'Add Workspace' })

  // Wait for the initial app state to settle into either the editor, the
  // loaded app shell, the welcome screen, or the welcome fallback dialog.
  await Promise.race([
    editor.first().waitFor({ state: 'visible', timeout: timeoutMs }),
    appSurface.waitFor({ state: 'visible', timeout: timeoutMs }),
    welcomeHeading.waitFor({ state: 'visible', timeout: timeoutMs }),
    addWorkspaceDialog.waitFor({ state: 'visible', timeout: timeoutMs }),
  ])

  // If the welcome screen appeared, select "More options" → "Minimal" bundle
  // to create a workspace without plugins for faster, more reliable E2E tests.
  if (await welcomeHeading.isVisible().catch(() => false)) {
    const moreOptionsButton = page.getByRole('button', { name: /more options/i })
    await expect(moreOptionsButton).toBeVisible({ timeout: 5000 })
    await moreOptionsButton.click()

    // Wait for bundle picker view and select the minimal bundle
    const minimalBundleButton = page.getByRole('button', { name: /minimal/i }).first()
    await expect(minimalBundleButton).toBeVisible({ timeout: 5000 })
    await minimalBundleButton.click()

    // Click "Get Started with Minimal" (or similar)
    const getStartedButton = page.getByRole('button', { name: /get started/i })
    await expect(getStartedButton).toBeVisible({ timeout: 5000 })
    await getStartedButton.click()

    await Promise.race([
      editor.first().waitFor({ state: 'visible', timeout: timeoutMs }),
      appSurface.waitFor({ state: 'visible', timeout: timeoutMs }),
      addWorkspaceDialog.waitFor({ state: 'visible', timeout: timeoutMs }),
    ])
  }

  // New onboarding can fall back to the Add Workspace dialog if auto-create
  // does not complete. Finish that flow here so tests still land in the editor.
  if (await addWorkspaceDialog.isVisible().catch(() => false)) {
    await completeAddWorkspaceDialog(page, timeoutMs)
  }

  await Promise.race([
    editor.first().waitFor({ state: 'visible', timeout: timeoutMs }),
    appSurface.waitFor({ state: 'visible', timeout: timeoutMs }),
  ])
}

/**
 * Wait for the app to be fully initialized.
 * Handles the welcome screen onboarding if no workspaces exist.
 */
export async function waitForAppReady(page: Page, timeoutMs: number = 20000): Promise<void> {
  // Wait for the main app container
  await page.waitForSelector('body', { state: 'visible' })

  // Handle welcome screen if it appears (first run / cleared storage)
  try {
    await handleWelcomeScreenIfNeeded(page, timeoutMs)
  } catch (error) {
    const welcomeHeading = page.getByRole('heading', { name: 'Welcome to Diaryx' })
    const addWorkspaceDialog = page.getByRole('dialog', { name: 'Add Workspace' })
    const needsBootstrapFallback =
      await welcomeHeading.isVisible().catch(() => false)
      || await addWorkspaceDialog.isVisible().catch(() => false)

    if (!needsBootstrapFallback) {
      throw error
    }

    await bootstrapWorkspaceFallback(page)
    await handleWelcomeScreenIfNeeded(page, timeoutMs)
  }

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
