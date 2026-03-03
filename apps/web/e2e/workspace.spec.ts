import { test, expect, EditorHelper, waitForAppReady } from './fixtures'

test.describe('Workspace', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/')
    await waitForAppReady(page)
  })

  test('should load the main page', async ({ page }) => {
    await expect(page.locator('body')).toBeVisible()
    const bodyContent = await page.locator('body').textContent()
    expect(bodyContent).toBeTruthy()
  })

  test('should display the editor', async ({ page }) => {
    const editor = page.locator('.ProseMirror, [contenteditable="true"]').first()
    await expect(editor).toBeVisible()
  })

  test('should display the right sidebar', async ({ page }) => {
    // The right sidebar should be visible with the properties content.
    // Tab bar is hidden when there is only one tab (no plugins loaded).
    const sidebar = page.locator('aside.border-l')
    await expect(sidebar).toBeVisible()

    // Properties content should be shown by default
    const addPropertyButton = page.locator('aside').locator('button').filter({ hasText: 'Add Property' })
    await expect(addPropertyButton).toBeVisible()
  })

  test('should show properties panel by default', async ({ page }) => {
    // Without plugins, the right sidebar defaults to the properties view
    const hasProperties = await page.locator('text=Add Property').isVisible()
    expect(hasProperties).toBe(true)
  })

  test('should collapse right sidebar when clicking collapse button', async ({ page }) => {
    const collapseButton = page.locator('button[aria-label="Collapse panel"]')
    await expect(collapseButton).toBeVisible()

    const sidebar = page.locator('aside.border-l')
    await expect(sidebar).toBeVisible()

    const initialWidth = await sidebar.evaluate(el => el.getBoundingClientRect().width)
    expect(initialWidth).toBeGreaterThan(0)

    await collapseButton.click()

    // Wait for collapse animation using explicit state check instead of arbitrary timeout
    await expect(async () => {
      const hasCollapsedClass = await sidebar.evaluate(el => el.classList.contains('w-0'))
      const collapsedWidth = await sidebar.evaluate(el => el.getBoundingClientRect().width)
      expect(hasCollapsedClass || collapsedWidth < initialWidth / 2).toBe(true)
    }).toPass({ timeout: 2000 })
  })
})

test.describe('Workspace Entry Properties', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/')
    await waitForAppReady(page)

    const propsTab = page.locator('button').filter({ hasText: 'Props' })
    await propsTab.click()
  })

  test('should show attachments section in properties', async ({ page }) => {
    const attachmentsHeader = page.locator('aside span.font-medium').filter({ hasText: 'Attachments' })
    await expect(attachmentsHeader).toBeVisible()
  })

  test('should show Add Property button', async ({ page }) => {
    const addPropertyButton = page.locator('aside').locator('button').filter({ hasText: 'Add Property' })
    await expect(addPropertyButton).toBeVisible()
  })

  test('should open add property form when clicking Add Property', async ({ page }) => {
    const addPropertyButton = page.locator('aside').locator('button').filter({ hasText: 'Add Property' })
    await addPropertyButton.click()

    const propertyNameInput = page.locator('input[placeholder="Property name..."]')
    const propertyValueInput = page.locator('input[placeholder="Value..."]')

    await expect(propertyNameInput).toBeVisible()
    await expect(propertyValueInput).toBeVisible()
  })
})

test.describe('Workspace Navigation', () => {
  test('should allow keyboard navigation in editor', async ({ page, editorHelper }) => {
    await page.goto('/')
    await waitForAppReady(page)
    await editorHelper.waitForReady()

    await editorHelper.focus()
    await editorHelper.clearContent()

    // Wait for editor to settle after clearing
    await expect(async () => {
      const text = await editorHelper.editor.textContent()
      expect((text || '').trim().length).toBeLessThanOrEqual(1)
    }).toPass({ timeout: 3000 })

    await editorHelper.type('Line 1')
    await page.keyboard.press('Enter')
    await editorHelper.type('Line 2')

    await expect(editorHelper.editor).toContainText('Line 1')
    await expect(editorHelper.editor).toContainText('Line 2')

    // Brief wait for editor to render both paragraphs before navigating
    await page.waitForTimeout(100)

    await page.keyboard.press('ArrowUp')
    await page.keyboard.press('End')
    await editorHelper.type(' more')

    await expect(editorHelper.editor).toContainText('Line 1 more')
  })
})
