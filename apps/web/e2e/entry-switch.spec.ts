import { test, expect, waitForAppReady } from './fixtures'

type SwitchFixture = {
  slowStem: string
  targetStem: string
  targetMarker: string
}

async function installAttachmentReadDelay(page: import('@playwright/test').Page, delayMs: number): Promise<void> {
  for (let attempt = 0; attempt < 3; attempt++) {
    try {
      await page.evaluate(async (delay) => {
        const { getBackend } = await import('/src/lib/backend')
        const backend = await getBackend()
        const g = globalThis as any

        if (g.__e2e_original_execute) return

        g.__e2e_original_execute = backend.execute.bind(backend)
        g.__e2e_inflight_attachment_reads = 0

        backend.execute = async (command: any) => {
          // Intercept FileExists calls for attachment probing —
          // the RightSidebar checks availability via fileExists for each attachment.
          if (command?.type === 'FileExists') {
            g.__e2e_inflight_attachment_reads += 1
            try {
              await new Promise((resolve) => setTimeout(resolve, delay))
              return await g.__e2e_original_execute(command)
            } finally {
              g.__e2e_inflight_attachment_reads -= 1
            }
          }

          return g.__e2e_original_execute(command)
        }
      }, delayMs)
      return
    } catch (e) {
      if (attempt === 2) throw e
      await page.waitForTimeout(200)
    }
  }
}

async function createEntrySwitchFixture(page: import('@playwright/test').Page): Promise<SwitchFixture> {
  return page.evaluate(async () => {
    const { getBackend, createApi } = await import('/src/lib/backend')
    const { refreshTree } = await import('/src/controllers/workspaceController')
    const { workspaceStore } = await import('/src/models/stores')

    const backend = await getBackend()
    const api = createApi(backend)

    const workspaceDir = backend
      .getWorkspacePath()
      .replace(/\/index\.md$/, '')
      .replace(/\/README\.md$/, '')

    let rootIndexPath: string
    try {
      rootIndexPath = await api.findRootIndex(workspaceDir)
    } catch {
      rootIndexPath = `${workspaceDir}/README.md`
    }

    const uid = `${Date.now()}-${Math.floor(Math.random() * 100000)}`
    const slowStem = `slow-switch-${uid}-slow`
    const targetStem = `slow-switch-${uid}-target`
    const slowPath = `${slowStem}.md`
    const targetPath = `${targetStem}.md`
    const targetMarker = `TARGET_SWITCH_MARKER_${uid}`

    // Create fake attachment paths in frontmatter so the RightSidebar
    // probes each one via GetAttachmentData (which the delay intercepts).
    const fakeAttachments = Array.from(
      { length: 60 },
      (_, i) => `${slowStem}-image-${i}.png`,
    )

    await api.createEntry(slowPath, { part_of: rootIndexPath })
    await api.saveEntry(slowPath, `# Slow entry\n\nLoading...`, rootIndexPath)
    await api.setFrontmatterProperty(slowPath, 'attachments', fakeAttachments)
    await api.createEntry(targetPath, { part_of: rootIndexPath })
    await api.saveEntry(targetPath, `# Target\n\n${targetMarker}`, rootIndexPath)

    try {
      const rootFrontmatter = await api.getFrontmatter(rootIndexPath)
      const nextContents = Array.isArray(rootFrontmatter.contents)
        ? [...rootFrontmatter.contents]
        : []

      if (!nextContents.includes(slowPath)) nextContents.push(slowPath)
      if (!nextContents.includes(targetPath)) nextContents.push(targetPath)

      await api.setFrontmatterProperty(rootIndexPath, 'contents', nextContents)
    } catch {
      // The entries are still valid via part_of; skip if root frontmatter is unavailable.
    }

    await refreshTree(
      api,
      backend,
      workspaceStore.showUnlinkedFiles,
      workspaceStore.showHiddenFiles,
      workspaceStore.currentAudience,
    )

    return {
      slowStem,
      targetStem,
      targetMarker,
    }
  })
}

async function ensureFilesTreeVisible(page: import('@playwright/test').Page): Promise<void> {
  const tree = page.getByRole('tree', { name: 'Workspace entries' })
  if (await tree.isVisible().catch(() => false)) return

  const filesTab = page.getByRole('button', { name: /^Files$/ }).first()
  if (await filesTab.isVisible().catch(() => false)) {
    await filesTab.click()
  }

  if (await tree.isVisible().catch(() => false)) return

  const openSidebar = page
    .locator('button[aria-label="Open navigation sidebar"], button[aria-label="Toggle navigation"]')
    .first()
  if (await openSidebar.isVisible().catch(() => false)) {
    await openSidebar.click()
  }

  await expect(tree).toBeVisible()
}

test.describe('Entry Switching', () => {
  test('switches to another entry immediately while attachments are still loading', async ({ page }) => {
    await page.addInitScript(() => {
      localStorage.setItem('diaryx-storage-type', 'indexeddb')
    })

    await page.goto('/')
    await waitForAppReady(page, 45000)

    await ensureFilesTreeVisible(page)
    await installAttachmentReadDelay(page, 2200)

    const fixture = await createEntrySwitchFixture(page)

    const slowTreeItem = page.getByRole('treeitem', {
      name: new RegExp(fixture.slowStem),
    })
    const targetTreeItem = page.getByRole('treeitem', {
      name: new RegExp(fixture.targetStem),
    })

    await expect(slowTreeItem).toBeVisible({ timeout: 15000 })
    await expect(targetTreeItem).toBeVisible({ timeout: 15000 })

    await slowTreeItem.locator('button').first().click()
    await expect(slowTreeItem).toHaveAttribute('aria-selected', 'true')

    await expect
      .poll(
        async () =>
          page.evaluate(
            () => ((globalThis as any).__e2e_inflight_attachment_reads ?? 0) as number,
          ),
        { timeout: 10000 },
      )
      .toBeGreaterThan(0)

    const startedAt = Date.now()
    await targetTreeItem.locator('button').first().click()
    await expect(targetTreeItem).toHaveAttribute('aria-selected', 'true', { timeout: 500 })
    const elapsedMs = Date.now() - startedAt
    expect(elapsedMs).toBeLessThan(500)

    await expect(page.locator('.ProseMirror, [contenteditable="true"]').first()).toContainText(
      fixture.targetMarker,
      { timeout: 5000 },
    )
  })
})
