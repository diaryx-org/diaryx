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
    await this.page.evaluate(() => {
      const editor = (globalThis as any).__diaryx_tiptapEditor
      if (editor) {
        editor.commands.undo()
      }
    })
  }

  async redo(): Promise<void> {
    await this.page.evaluate(() => {
      const editor = (globalThis as any).__diaryx_tiptapEditor
      if (editor) {
        editor.commands.redo()
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
    // Click "Get Started" to open the AddWorkspaceDialog
    const getStartedButton = page.getByRole('button', { name: 'Get Started' })
    await expect(getStartedButton).toBeVisible({ timeout: 5000 })
    await getStartedButton.click()

    // Click "Create Workspace" in the AddWorkspaceDialog
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
