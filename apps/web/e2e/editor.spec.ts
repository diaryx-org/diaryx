import { test, expect, EditorHelper, waitForAppReady } from './fixtures'

test.describe('Editor', () => {
  let editor: EditorHelper

  test.beforeEach(async ({ page }) => {
    await page.goto('/')
    await waitForAppReady(page)
    editor = new EditorHelper(page)
    await editor.waitForReady()
  })

  test('should display editor when entry is loaded', async () => {
    await expect(editor.editor).toBeVisible()
  })

  test('should allow typing in the editor', async ({ page }) => {
    await editor.focus()
    await editor.type('Test content')
    await expect(editor.editor).toContainText('Test content')
  })

  test('should apply bold formatting with keyboard shortcut', async ({ page }) => {
    await editor.focus()
    await editor.type('Bold text')
    await editor.selectAll()
    await editor.applyBold()

    // Wait for formatting to apply by checking for the styled element
    const hasBold = await editor.editor.evaluate((el) => {
      const strong = el.querySelector('strong')
      if (strong) return true
      const p = el.querySelector('p')
      if (p) {
        const weight = window.getComputedStyle(p).fontWeight
        return parseInt(weight) >= 600
      }
      return false
    })
    expect(hasBold).toBe(true)
  })

  test('should apply italic formatting with keyboard shortcut', async ({ page }) => {
    await editor.focus()
    await editor.type('Italic text')
    await editor.selectAll()
    await editor.applyItalic()

    const hasItalic = await editor.editor.evaluate((el) => {
      const em = el.querySelector('em')
      if (em) return true
      const p = el.querySelector('p')
      if (p) {
        const style = window.getComputedStyle(p).fontStyle
        return style === 'italic'
      }
      return false
    })
    expect(hasItalic).toBe(true)
  })

  test('should create a heading via floating menu', async ({ page }) => {
    await editor.expandFloatingMenu()

    // Open heading submenu and select H1
    const headingButton = page.locator('.block-picker-menu .menu-item[title="Heading"]')
    await headingButton.click()
    const h1Button = page.locator('.block-picker-menu .submenu-item').filter({ hasText: 'H1' })
    await h1Button.click()

    await page.keyboard.type('My Heading')

    const heading = editor.editor.locator('h1')
    await expect(heading).toContainText('My Heading')
  })

  test('should create a bulleted list via floating menu', async ({ page }) => {
    await editor.expandFloatingMenu()

    // Open list submenu and select bullet list
    const listButton = page.locator('.block-picker-menu .menu-item[title="List"]')
    await listButton.click()
    const bulletButton = page.locator('.block-picker-menu .submenu-item').filter({ hasText: 'Bullet' })
    await bulletButton.click()

    await page.keyboard.type('Item 1')
    await page.keyboard.press('Enter')
    await page.keyboard.type('Item 2')

    const list = editor.editor.locator('ul')
    await expect(list).toBeVisible()
    const items = editor.editor.locator('li')
    await expect(items).toHaveCount(2)
  })

  test('should create a numbered list via floating menu', async ({ page }) => {
    await editor.expandFloatingMenu()

    // Open list submenu and select numbered list
    const listButton = page.locator('.block-picker-menu .menu-item[title="List"]')
    await listButton.click()
    const numberedButton = page.locator('.block-picker-menu .submenu-item').filter({ hasText: 'Numbered' })
    await numberedButton.click()

    await page.keyboard.type('First')
    await page.keyboard.press('Enter')
    await page.keyboard.type('Second')

    const orderedList = editor.editor.locator('ol')
    await expect(orderedList).toBeVisible()
  })

  test('should create a task list via floating menu', async ({ page }) => {
    await editor.expandFloatingMenu()

    // Open list submenu and select task list
    const listButton = page.locator('.block-picker-menu .menu-item[title="List"]')
    await listButton.click()
    const taskButton = page.locator('.block-picker-menu .submenu-item').filter({ hasText: 'Task' })
    await taskButton.click()

    await page.keyboard.type('My task')

    const taskList = editor.editor.locator('ul[data-type="taskList"]')
    await expect(taskList).toBeVisible()
  })

  test('should create a code block via floating menu', async ({ page }) => {
    await editor.expandFloatingMenu()

    const codeBlockButton = page.locator('.block-picker-menu .menu-item[title="Code Block"]')
    await codeBlockButton.click()

    await page.keyboard.type('const x = 1;')

    const codeBlock = editor.editor.locator('pre')
    await expect(codeBlock).toBeVisible()
    await expect(codeBlock).toContainText('const x = 1')
  })

  test('should create a blockquote via floating menu', async ({ page }) => {
    await editor.expandFloatingMenu()

    const quoteButton = page.locator('.block-picker-menu .menu-item[title="Quote"]')
    await quoteButton.click()

    await page.keyboard.type('A famous quote')

    const blockquote = editor.editor.locator('blockquote')
    await expect(blockquote).toBeVisible()
    await expect(blockquote).toContainText('A famous quote')
  })
})

test.describe('Editor Keyboard Navigation', () => {
  test('should support undo', async ({ page, editorHelper }) => {
    await page.goto('/')
    await waitForAppReady(page)
    await editorHelper.waitForReady()

    const initialLength = (await editorHelper.editor.textContent())?.length || 0

    await editorHelper.focus()
    await editorHelper.type('Hello')
    await expect(editorHelper.editor).toContainText('Hello')

    const afterTypingLength = (await editorHelper.editor.textContent())?.length || 0
    expect(afterTypingLength).toBeGreaterThan(initialLength)

    await editorHelper.undo()

    // Wait for undo to take effect
    await expect(async () => {
      const afterUndoLength = (await editorHelper.editor.textContent())?.length || 0
      expect(afterUndoLength).toBeLessThan(afterTypingLength)
    }).toPass({ timeout: 3000 })
  })

  test('should support redo', async ({ page, editorHelper }) => {
    await page.goto('/')
    await waitForAppReady(page)
    await editorHelper.waitForReady()

    const initialLength = (await editorHelper.editor.textContent())?.length || 0

    await editorHelper.focus()
    await editorHelper.type('Test')

    const afterTypingLength = (await editorHelper.editor.textContent())?.length || 0
    expect(afterTypingLength).toBeGreaterThan(initialLength)

    await editorHelper.undo()

    // Wait for undo
    await expect(async () => {
      const len = (await editorHelper.editor.textContent())?.length || 0
      expect(len).toBeLessThan(afterTypingLength)
    }).toPass({ timeout: 3000 })

    await editorHelper.redo()

    // Wait for redo
    await expect(async () => {
      const afterRedoLength = (await editorHelper.editor.textContent())?.length || 0
      expect(afterRedoLength).toBe(afterTypingLength)
    }).toPass({ timeout: 3000 })
  })
})

test.describe('Editor Floating Menu', () => {
  test('should show floating menu on empty line', async ({ page, editorHelper }) => {
    await page.goto('/')
    await waitForAppReady(page)
    await editorHelper.waitForReady()

    await editorHelper.focus()
    await editorHelper.type('temp text')
    await editorHelper.clearContent()

    const floatingMenu = page.locator('.floating-menu')
    await expect(floatingMenu).toBeVisible()
  })

  test('should expand menu when clicking plus button', async ({ page, editorHelper }) => {
    await page.goto('/')
    await waitForAppReady(page)
    await editorHelper.waitForReady()

    const plusButton = await editorHelper.openFloatingMenu()
    await plusButton.click()

    const blockPicker = page.locator('.block-picker-menu')
    await expect(blockPicker).toBeVisible()
  })

  test('should close block picker with Escape', async ({ page, editorHelper }) => {
    await page.goto('/')
    await waitForAppReady(page)
    await editorHelper.waitForReady()

    await editorHelper.expandFloatingMenu()

    const blockPicker = page.locator('.block-picker-menu')
    await expect(blockPicker).toBeVisible()

    await page.keyboard.press('Escape')

    await expect(blockPicker).not.toBeVisible()
  })
})
