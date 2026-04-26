<script lang="ts">
  /**
   * Master-detail editor for the workspace's `audiences:` declaration.
   *
   * Source of truth: the root index frontmatter, read + written through
   * `getWorkspaceConfigStore()`. The dialog keeps a working copy of the
   * declared audiences that is committed back to the file via
   * `setField('audiences', JSON.stringify(...))` — coarse changes
   * (add/delete) save immediately; per-audience field edits batch under an
   * explicit Save button so a half-edited audience can never reach the
   * file.
   */

  import { untrack } from 'svelte';
  import { Button } from '$lib/components/ui/button';
  import * as Dialog from '$lib/components/ui/dialog';
  import { Input } from '$lib/components/ui/input';
  import { Label } from '$lib/components/ui/label';
  import { Switch } from '$lib/components/ui/switch';
  import {
    Plus,
    Trash2,
    Mail,
    Link as LinkIcon,
    Lock,
    Globe,
    Save,
    Undo2,
    AlertTriangle,
  } from '@lucide/svelte';
  import { showError, showSuccess } from '@/models/services/toastService';
  import { getWorkspaceConfigStore } from '$lib/stores/workspaceConfigStore.svelte';
  import type {
    AudienceDecl,
    Gate,
    ShareAction,
  } from '$lib/backend/generated';

  interface Props {
    open: boolean;
    /** Initial audience name to focus when opening. Optional. */
    initialAudience?: string | null;
    /** Called when the dialog wants to close. */
    onOpenChange: (open: boolean) => void;
  }

  let { open, initialAudience = null, onOpenChange }: Props = $props();

  const configStore = getWorkspaceConfigStore();

  // ==========================================================================
  // Working state
  // ==========================================================================

  // Last-committed snapshot, used to compute dirty-state.
  let originalDecls = $state<AudienceDecl[]>([]);
  // Working copy. Edits flow into this; saved per-audience.
  let decls = $state<AudienceDecl[]>([]);
  let selectedIndex = $state<number | null>(null);
  let isSaving = $state(false);

  // Confirm dialogs.
  let confirmOpen = $state(false);
  let confirmTitle = $state('');
  let confirmMessage = $state('');
  let confirmAction = $state<(() => void) | null>(null);
  let confirmDestructive = $state(false);

  // ==========================================================================
  // Effects: hydrate working copy on the closed→open transition only.
  //
  // The effect must depend on `open` (and only `open`); the actual hydration
  // writes to `decls`/`originalDecls`/`selectedIndex`, which are written
  // again by editing handlers — re-reading them inside this effect would
  // cause an infinite loop. `untrack` isolates the writes from the
  // dependency graph.
  // ==========================================================================

  let wasOpen = false;
  $effect(() => {
    const isOpen = open;
    if (isOpen && !wasOpen) {
      untrack(() => hydrate());
    }
    wasOpen = isOpen;
  });

  function hydrate() {
    const fileDecls = configStore.config?.audiences ?? [];
    // Deep clone via JSON so editing the working copy can't bleed into the
    // store's value before Save.
    originalDecls = JSON.parse(JSON.stringify(fileDecls));
    decls = JSON.parse(JSON.stringify(fileDecls));
    if (initialAudience) {
      const idx = decls.findIndex((d) => d.name === initialAudience);
      selectedIndex = idx === -1 ? (decls.length > 0 ? 0 : null) : idx;
    } else if (decls.length > 0) {
      selectedIndex = 0;
    } else {
      selectedIndex = null;
    }
  }

  // ==========================================================================
  // Dirty-state tracking (per-audience)
  // ==========================================================================

  function isDirty(index: number): boolean {
    const a = decls[index];
    const b = originalDecls[index];
    if (!a || !b) return false;
    return JSON.stringify(a) !== JSON.stringify(b);
  }

  const selectedDirty = $derived(
    selectedIndex !== null && isDirty(selectedIndex),
  );
  const anyDirty = $derived(decls.some((_, i) => isDirty(i)));

  // ==========================================================================
  // Persistence helpers
  // ==========================================================================

  /** Write the full working copy to the workspace file. */
  async function persist(updated: AudienceDecl[]): Promise<boolean> {
    if (!configStore.rootIndexPath) {
      showError('No workspace open.', 'Audiences');
      return false;
    }
    isSaving = true;
    try {
      await configStore.setField('audiences', JSON.stringify(updated));
      // After persist, treat the persisted shape as the new baseline.
      originalDecls = JSON.parse(JSON.stringify(updated));
      decls = JSON.parse(JSON.stringify(updated));
      return true;
    } catch (e) {
      showError(
        e instanceof Error ? e.message : 'Failed to save audiences',
        'Audiences',
      );
      return false;
    } finally {
      isSaving = false;
    }
  }

  // ==========================================================================
  // Add / delete (immediate-save, coarse intents)
  // ==========================================================================

  function uniqueDefaultName(): string {
    const existing = new Set(decls.map((d) => d.name.toLowerCase()));
    let i = 1;
    while (existing.has(`audience-${i}`.toLowerCase())) i += 1;
    return `audience-${i}`;
  }

  async function addAudience() {
    if (selectedDirty) {
      askConfirm({
        title: 'Discard unsaved changes?',
        message:
          'Adding a new audience saves the file immediately, which would discard the in-progress edits to the current audience.',
        destructive: true,
        onConfirm: async () => {
          // Discard current edits first by reverting to original.
          decls = JSON.parse(JSON.stringify(originalDecls));
          await reallyAdd();
        },
      });
      return;
    }
    await reallyAdd();
  }

  async function reallyAdd() {
    const next: AudienceDecl[] = [
      ...decls,
      { name: uniqueDefaultName(), gates: [], share_actions: [] },
    ];
    const ok = await persist(next);
    if (ok) {
      selectedIndex = next.length - 1;
      showSuccess('Audience added.');
    }
  }

  function deleteAudience(index: number) {
    const decl = decls[index];
    if (!decl) return;
    const hasPassword = decl.gates.some((g) => g.kind === 'password');
    const hasLink = decl.gates.some((g) => g.kind === 'link');
    const consequences: string[] = [];
    if (hasPassword) {
      consequences.push(
        'Anyone with a password unlock cookie will lose access immediately.',
      );
    }
    if (hasLink) {
      consequences.push(
        'Magic links you have shared will stop working on the next publish.',
      );
    }
    askConfirm({
      title: `Delete "${decl.name}"?`,
      message: [
        'This removes the audience from the workspace file.',
        ...consequences,
        'Entries tagged with this audience name will no longer publish to it on the next publish.',
      ].join(' '),
      destructive: true,
      onConfirm: async () => {
        const next = decls.filter((_, i) => i !== index);
        const ok = await persist(next);
        if (ok) {
          if (next.length === 0) {
            selectedIndex = null;
          } else if (selectedIndex !== null && selectedIndex >= next.length) {
            selectedIndex = next.length - 1;
          }
          showSuccess('Audience deleted.');
        }
      },
    });
  }

  // ==========================================================================
  // Per-audience edits (working-copy mutations; require explicit Save)
  // ==========================================================================

  function selectAudience(index: number) {
    if (index === selectedIndex) return;
    if (selectedDirty) {
      askConfirm({
        title: 'Discard unsaved changes?',
        message:
          'Switching to a different audience without saving will discard the edits in progress.',
        destructive: true,
        onConfirm: () => {
          // Roll back working copy to original for the dirty audience.
          if (selectedIndex !== null) {
            decls = decls.map((d, i) =>
              i === selectedIndex
                ? JSON.parse(JSON.stringify(originalDecls[i]))
                : d,
            );
          }
          selectedIndex = index;
        },
      });
      return;
    }
    selectedIndex = index;
  }

  function discardChanges() {
    if (selectedIndex === null) return;
    decls = decls.map((d, i) =>
      i === selectedIndex
        ? JSON.parse(JSON.stringify(originalDecls[i]))
        : d,
    );
  }

  async function saveCurrent() {
    if (selectedIndex === null) return;
    // Validate: name uniqueness, non-empty.
    const decl = decls[selectedIndex];
    if (!decl.name.trim()) {
      showError('Audience name is required.', 'Audiences');
      return;
    }
    const dupIdx = decls.findIndex(
      (d, i) => i !== selectedIndex && d.name.toLowerCase() === decl.name.toLowerCase(),
    );
    if (dupIdx !== -1) {
      showError(
        `Another audience is already named "${decl.name}".`,
        'Audiences',
      );
      return;
    }
    const ok = await persist(decls);
    if (ok) showSuccess(`Saved "${decl.name}".`);
  }

  function setSelectedDecl(updater: (d: AudienceDecl) => AudienceDecl) {
    if (selectedIndex === null) return;
    decls = decls.map((d, i) => (i === selectedIndex ? updater(d) : d));
  }

  // --- gates ----------------------------------------------------------------

  function hasGate(decl: AudienceDecl, kind: Gate['kind']): boolean {
    return decl.gates.some((g) => g.kind === kind);
  }

  function toggleGate(kind: Gate['kind'], on: boolean) {
    if (selectedIndex === null) return;
    const decl = decls[selectedIndex];
    const has = hasGate(decl, kind);
    if (on === has) return;

    if (!on) {
      // Removing — destructive enough to confirm, especially against the
      // original (saved) state. Pre-existing gates being removed will
      // invalidate live tokens on next publish.
      const original = originalDecls[selectedIndex];
      const wasOriginallyOn = original
        ? hasGate(original, kind)
        : false;
      if (wasOriginallyOn) {
        const consequence =
          kind === 'link'
            ? 'Existing magic links for this audience will stop working on the next publish.'
            : 'Anyone using a password to read this audience will be locked out on the next publish.';
        askConfirm({
          title: `Remove ${kind === 'link' ? 'magic-link' : 'password'} gate?`,
          message: `${consequence} You can re-add it later, but old credentials are not restored.`,
          destructive: true,
          onConfirm: () => {
            setSelectedDecl((d) => ({
              ...d,
              gates: d.gates.filter((g) => g.kind !== kind),
            }));
          },
        });
        return;
      }
      // Toggling off a gate that wasn't there originally is just a no-op
      // edit — no confirmation needed.
      setSelectedDecl((d) => ({
        ...d,
        gates: d.gates.filter((g) => g.kind !== kind),
      }));
    } else {
      // Adding — straightforward.
      const next: Gate = kind === 'link' ? { kind: 'link' } : { kind: 'password' };
      setSelectedDecl((d) => ({
        ...d,
        gates: [...d.gates, next],
      }));
    }
  }

  // --- share actions --------------------------------------------------------

  function addShareAction(kind: ShareAction['kind']) {
    if (selectedIndex === null) return;
    const action: ShareAction =
      kind === 'email'
        ? { kind: 'email', recipients: [] }
        : { kind: 'copy_link' };
    setSelectedDecl((d) => ({
      ...d,
      share_actions: [...d.share_actions, action],
    }));
  }

  function removeShareAction(actionIndex: number) {
    setSelectedDecl((d) => ({
      ...d,
      share_actions: d.share_actions.filter((_, i) => i !== actionIndex),
    }));
  }

  function updateShareAction(actionIndex: number, next: ShareAction) {
    setSelectedDecl((d) => ({
      ...d,
      share_actions: d.share_actions.map((a, i) => (i === actionIndex ? next : a)),
    }));
  }

  // --- recipient chip editor (inline) ---------------------------------------

  let recipientInputs = $state<Record<number, string>>({});

  function addRecipient(actionIndex: number, raw: string) {
    const trimmed = raw.trim().replace(/,$/, '').trim();
    if (!trimmed) return;
    const action = decls[selectedIndex!]?.share_actions[actionIndex];
    if (!action || action.kind !== 'email') return;
    if (action.recipients.includes(trimmed)) {
      recipientInputs[actionIndex] = '';
      return;
    }
    updateShareAction(actionIndex, {
      ...action,
      recipients: [...action.recipients, trimmed],
    });
    recipientInputs[actionIndex] = '';
  }

  function handleRecipientPaste(event: ClipboardEvent, actionIndex: number) {
    const text = event.clipboardData?.getData('text');
    if (!text || !text.includes(',')) return;
    event.preventDefault();
    const action = decls[selectedIndex!]?.share_actions[actionIndex];
    if (!action || action.kind !== 'email') return;
    const parts = text
      .split(/[,\n;]/)
      .map((s) => s.trim())
      .filter((s) => s.length > 0)
      .filter((s) => !action.recipients.includes(s));
    if (parts.length === 0) return;
    updateShareAction(actionIndex, {
      ...action,
      recipients: [...action.recipients, ...parts],
    });
    recipientInputs[actionIndex] = '';
  }

  function handleRecipientKeydown(
    event: KeyboardEvent,
    actionIndex: number,
  ) {
    const target = event.currentTarget as HTMLInputElement;
    if (event.key === 'Enter' || event.key === ',') {
      event.preventDefault();
      addRecipient(actionIndex, target.value);
    } else if (event.key === 'Backspace' && target.value === '') {
      const action = decls[selectedIndex!]?.share_actions[actionIndex];
      if (!action || action.kind !== 'email') return;
      if (action.recipients.length === 0) return;
      event.preventDefault();
      updateShareAction(actionIndex, {
        ...action,
        recipients: action.recipients.slice(0, -1),
      });
    }
  }

  function removeRecipient(actionIndex: number, recipient: string) {
    const action = decls[selectedIndex!]?.share_actions[actionIndex];
    if (!action || action.kind !== 'email') return;
    updateShareAction(actionIndex, {
      ...action,
      recipients: action.recipients.filter((r) => r !== recipient),
    });
  }

  function isValidEmail(value: string): boolean {
    // Simple shape check — not a full RFC 5322 parser. Just enough to flag
    // obvious typos without rejecting legitimate addresses.
    return /^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(value);
  }

  // ==========================================================================
  // Confirm dialog plumbing
  // ==========================================================================

  function askConfirm(args: {
    title: string;
    message: string;
    destructive?: boolean;
    onConfirm: () => void | Promise<void>;
  }) {
    confirmTitle = args.title;
    confirmMessage = args.message;
    confirmDestructive = args.destructive ?? false;
    confirmAction = () => {
      args.onConfirm();
    };
    confirmOpen = true;
  }

  function runConfirm() {
    confirmAction?.();
    confirmAction = null;
    confirmOpen = false;
  }

  // ==========================================================================
  // Close handling
  // ==========================================================================

  function requestClose(next: boolean) {
    if (!next && anyDirty) {
      askConfirm({
        title: 'Discard unsaved changes?',
        message:
          'Closing the editor without saving discards in-progress edits. Add/delete actions are already saved.',
        destructive: true,
        onConfirm: () => onOpenChange(false),
      });
      return;
    }
    onOpenChange(next);
  }

  // ==========================================================================
  // Derived: currently-selected audience for cleaner template binding
  // ==========================================================================

  const selectedDecl = $derived(
    selectedIndex !== null ? decls[selectedIndex] ?? null : null,
  );
</script>

<Dialog.Root open={open} onOpenChange={requestClose}>
  <Dialog.Content
    class="sm:max-w-3xl"
    style="height: min(90vh, 640px); display: flex; flex-direction: column;"
  >
    <Dialog.Header>
      <Dialog.Title>Audiences</Dialog.Title>
      <Dialog.Description>
        Declare who you share with and how. Saved to the workspace file.
      </Dialog.Description>
    </Dialog.Header>

    <div class="flex flex-1 gap-4 overflow-hidden">
      <!-- Master rail -->
      <div class="flex w-52 shrink-0 flex-col gap-1 overflow-y-auto border-r border-border pr-3">
        {#if decls.length === 0}
          <p class="px-1 py-2 text-xs text-muted-foreground">
            No audiences yet.
          </p>
        {/if}
        {#each decls as decl, i (i)}
          {@const dirty = isDirty(i)}
          <button
            type="button"
            class="group flex items-center gap-2 rounded-md border border-transparent px-2 py-1.5 text-left text-sm hover:bg-accent {selectedIndex ===
            i
              ? 'border-border bg-accent'
              : ''}"
            onclick={() => selectAudience(i)}
          >
            <span class="flex-1 truncate">{decl.name}</span>
            {#if dirty}
              <span
                class="size-1.5 rounded-full bg-amber-500"
                title="Unsaved changes"
                aria-label="Unsaved changes"
              ></span>
            {/if}
          </button>
        {/each}

        <Button
          variant="outline"
          size="sm"
          class="mt-2"
          onclick={addAudience}
          disabled={isSaving}
        >
          <Plus class="mr-1.5 size-3.5" />
          Add audience
        </Button>
      </div>

      <!-- Detail pane -->
      <div class="flex flex-1 flex-col gap-3 overflow-y-auto pr-1">
        {#if !selectedDecl}
          <div class="flex flex-1 items-center justify-center">
            <p class="text-sm text-muted-foreground">
              Select an audience on the left, or click "Add audience" to
              create one.
            </p>
          </div>
        {:else}
          <!-- Name (rename disabled in v1) -->
          <div>
            <Label for="audience-name">Name</Label>
            <Input
              id="audience-name"
              type="text"
              value={selectedDecl.name}
              disabled
              title="Renaming an audience is not supported in v1 because entry frontmatter `audience:` tags reference the name. Delete this audience and create a new one if needed."
            />
            <p class="mt-1 text-[11px] text-muted-foreground">
              Renaming is disabled — entry tags reference this name. Delete
              and recreate to rename.
            </p>
          </div>

          <!-- Gates -->
          <fieldset class="space-y-2 rounded-md border border-border p-3">
            <legend class="px-1 text-xs font-medium text-muted-foreground">
              Access gates
            </legend>
            {#if selectedDecl.gates.length === 0}
              <p
                class="flex items-center gap-2 rounded bg-muted px-2 py-1.5 text-xs text-muted-foreground"
              >
                <Globe class="size-3.5" />
                Empty gate set — anyone with the URL can read.
              </p>
            {/if}

            <div class="flex items-center justify-between">
              <div class="flex items-center gap-2">
                <LinkIcon class="size-4 text-muted-foreground" />
                <div>
                  <div class="text-sm font-medium">Magic link</div>
                  <div class="text-[11px] text-muted-foreground">
                    Reader presents a signed link; bypasses other gates.
                  </div>
                </div>
              </div>
              <Switch
                checked={hasGate(selectedDecl, 'link')}
                onCheckedChange={(v) => toggleGate('link', v)}
              />
            </div>

            <div class="flex items-center justify-between">
              <div class="flex items-center gap-2">
                <Lock class="size-4 text-muted-foreground" />
                <div>
                  <div class="text-sm font-medium">Password</div>
                  <div class="text-[11px] text-muted-foreground">
                    Reader enters a password on the audience URL.
                  </div>
                </div>
              </div>
              <Switch
                checked={hasGate(selectedDecl, 'password')}
                onCheckedChange={(v) => toggleGate('password', v)}
              />
            </div>
          </fieldset>

          <!-- Share actions -->
          <fieldset class="space-y-2 rounded-md border border-border p-3">
            <legend class="px-1 text-xs font-medium text-muted-foreground">
              Share channels
            </legend>
            <p class="text-[11px] text-muted-foreground">
              Labeled shortcuts in the audience card. Templates support
              <code class="rounded bg-muted px-1">{'{{title}}'}</code>
              and
              <code class="rounded bg-muted px-1">{'{{url}}'}</code>.
            </p>

            {#if selectedDecl.share_actions.length === 0}
              <p class="text-xs text-muted-foreground">No share channels configured.</p>
            {/if}

            {#each selectedDecl.share_actions as action, actionIndex (actionIndex)}
              <div class="space-y-2 rounded border border-border bg-muted/30 p-2">
                <div class="flex items-center justify-between">
                  <span class="flex items-center gap-1.5 text-sm font-medium">
                    {#if action.kind === 'email'}
                      <Mail class="size-3.5" /> Email (BCC)
                    {:else}
                      <LinkIcon class="size-3.5" /> Copy link
                    {/if}
                  </span>
                  <Button
                    variant="ghost"
                    size="sm"
                    onclick={() => removeShareAction(actionIndex)}
                  >
                    <Trash2 class="size-3.5" />
                  </Button>
                </div>

                {#if action.kind === 'email'}
                  <div>
                    <Label for={`recipients-${actionIndex}`}>Recipients</Label>
                    <div
                      class="flex min-h-9 flex-wrap items-center gap-1 rounded-md border border-input bg-background px-2 py-1 text-sm"
                    >
                      {#each action.recipients as recipient}
                        <span
                          class="inline-flex items-center gap-1 rounded-full px-2 py-0.5 text-xs {isValidEmail(
                            recipient,
                          )
                            ? 'bg-muted'
                            : 'border border-destructive/40 bg-destructive/10 text-destructive'}"
                        >
                          {recipient}
                          <button
                            type="button"
                            class="opacity-60 hover:opacity-100"
                            onclick={() => removeRecipient(actionIndex, recipient)}
                            aria-label={`Remove ${recipient}`}
                          >
                            ×
                          </button>
                        </span>
                      {/each}
                      <input
                        id={`recipients-${actionIndex}`}
                        type="text"
                        class="flex-1 min-w-32 bg-transparent outline-none placeholder:text-muted-foreground"
                        placeholder="email@example.com"
                        value={recipientInputs[actionIndex] ?? ''}
                        oninput={(e) => {
                          recipientInputs[actionIndex] = e.currentTarget.value;
                        }}
                        onkeydown={(e) => handleRecipientKeydown(e, actionIndex)}
                        onpaste={(e) => handleRecipientPaste(e, actionIndex)}
                        onblur={(e) => addRecipient(actionIndex, e.currentTarget.value)}
                      />
                    </div>
                    <p class="mt-1 text-[11px] text-muted-foreground">
                      Press Enter or comma to add. Invalid addresses appear
                      red but aren't blocked.
                    </p>
                  </div>

                  <div>
                    <Label for={`subject-${actionIndex}`}>Subject template</Label>
                    <Input
                      id={`subject-${actionIndex}`}
                      type="text"
                      placeholder="New from me: {'{{title}}'}"
                      value={action.subject_template ?? ''}
                      oninput={(e) =>
                        updateShareAction(actionIndex, {
                          ...action,
                          subject_template: e.currentTarget.value || undefined,
                        })}
                    />
                  </div>

                  <div>
                    <Label for={`body-${actionIndex}`}>Body template</Label>
                    <textarea
                      id={`body-${actionIndex}`}
                      class="flex min-h-16 w-full rounded-md border border-input bg-background px-3 py-1.5 text-sm shadow-sm placeholder:text-muted-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                      placeholder={'New audience update — {{url}}'}
                      value={action.body_template ?? ''}
                      oninput={(e) =>
                        updateShareAction(actionIndex, {
                          ...action,
                          body_template: e.currentTarget.value || undefined,
                        })}
                    ></textarea>
                  </div>
                {:else}
                  <div>
                    <Label for={`label-${actionIndex}`}>Label</Label>
                    <Input
                      id={`label-${actionIndex}`}
                      type="text"
                      placeholder="For the group chat"
                      value={action.label ?? ''}
                      oninput={(e) =>
                        updateShareAction(actionIndex, {
                          kind: 'copy_link',
                          label: e.currentTarget.value || undefined,
                        })}
                    />
                  </div>
                {/if}
              </div>
            {/each}

            <div class="flex gap-2 pt-1">
              <Button
                variant="outline"
                size="sm"
                onclick={() => addShareAction('email')}
              >
                <Mail class="mr-1.5 size-3.5" /> Add email
              </Button>
              <Button
                variant="outline"
                size="sm"
                onclick={() => addShareAction('copy_link')}
              >
                <LinkIcon class="mr-1.5 size-3.5" /> Add copy-link
              </Button>
            </div>
          </fieldset>

          <!-- Per-audience footer: Save / Discard -->
          <div class="mt-1 flex items-center justify-between gap-2 border-t border-border pt-3">
            <Button
              variant="ghost"
              size="sm"
              class="text-destructive hover:text-destructive"
              onclick={() =>
                selectedIndex !== null && deleteAudience(selectedIndex)}
            >
              <Trash2 class="mr-1.5 size-3.5" /> Delete audience
            </Button>
            <div class="flex gap-2">
              <Button
                variant="outline"
                size="sm"
                onclick={discardChanges}
                disabled={!selectedDirty || isSaving}
              >
                <Undo2 class="mr-1.5 size-3.5" /> Discard
              </Button>
              <Button
                size="sm"
                onclick={saveCurrent}
                disabled={!selectedDirty || isSaving}
              >
                <Save class="mr-1.5 size-3.5" />
                Save
              </Button>
            </div>
          </div>
        {/if}
      </div>
    </div>

    <Dialog.Footer>
      <Button variant="outline" onclick={() => requestClose(false)}>
        Close
      </Button>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>

<!-- Confirm sub-dialog -->
<Dialog.Root open={confirmOpen} onOpenChange={(v) => (confirmOpen = v)}>
  <Dialog.Content class="sm:max-w-md">
    <Dialog.Header>
      <Dialog.Title class="flex items-center gap-2">
        {#if confirmDestructive}
          <AlertTriangle class="size-4 text-amber-600 dark:text-amber-400" />
        {/if}
        {confirmTitle}
      </Dialog.Title>
      <Dialog.Description>
        {confirmMessage}
      </Dialog.Description>
    </Dialog.Header>
    <Dialog.Footer>
      <Button variant="outline" onclick={() => (confirmOpen = false)}>
        Cancel
      </Button>
      <Button
        variant={confirmDestructive ? 'destructive' : 'default'}
        onclick={runConfirm}
      >
        Continue
      </Button>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>
