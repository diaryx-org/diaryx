/**
 * TipTap extension for Notion-style inline table controls.
 *
 * Renders an overlay with:
 * - Row grip handles (left of each row) with move/delete popover
 * - Column grip handles (above each column) with move/delete popover
 * - Add-row button (below table)
 * - Add-column button (right of table)
 *
 * Uses a ProseMirror plugin view so the controls are DOM overlays that
 * don't interfere with the table's own DOM structure.
 */

import { Extension } from "@tiptap/core";
import { Plugin, PluginKey, TextSelection } from "@tiptap/pm/state";
import type { EditorView } from "@tiptap/pm/view";
import type { Node as PmNode } from "@tiptap/pm/model";
import {
  TableMap,
  CellSelection,
  addColumnAfter,
  addRowAfter,
  deleteRow,
  deleteColumn,
  moveTableRow,
  moveTableColumn,
} from "@tiptap/pm/tables";

const TABLE_CONTROLS_KEY = new PluginKey("tableControls");

/* ------------------------------------------------------------------ */
/*  Helpers                                                           */
/* ------------------------------------------------------------------ */

/** Walk up from `$pos` to find the nearest ancestor table node. */
function findTable(state: EditorView["state"]) {
  const { $from } = state.selection;
  for (let depth = $from.depth; depth > 0; depth--) {
    const node = $from.node(depth);
    if (node.type.name === "table") {
      return { node, start: $from.before(depth), depth };
    }
  }
  return null;
}

/** Return { row, col } for the current cursor inside a table. */
function getCellIndex(
  state: EditorView["state"],
  tableStart: number,
  tableNode: PmNode,
) {
  const map = TableMap.get(tableNode);
  const cellPos = state.selection.$from.pos - tableStart;
  for (let row = 0; row < map.height; row++) {
    for (let col = 0; col < map.width; col++) {
      const idx = row * map.width + col;
      const mapped = map.map[idx];
      if (mapped === undefined) continue;
      const cellNode = tableNode.nodeAt(mapped);
      if (!cellNode) continue;
      const cellEnd = mapped + cellNode.nodeSize;
      if (cellPos >= mapped && cellPos < cellEnd) {
        return { row, col };
      }
    }
  }
  return { row: 0, col: 0 };
}

/* ------------------------------------------------------------------ */
/*  SVG icon data                                                     */
/* ------------------------------------------------------------------ */

const GRIP_DOTS_SVG = `<svg width="10" height="16" viewBox="0 0 10 16" fill="currentColor" xmlns="http://www.w3.org/2000/svg">
  <circle cx="3" cy="3" r="1.5"/>
  <circle cx="7" cy="3" r="1.5"/>
  <circle cx="3" cy="8" r="1.5"/>
  <circle cx="7" cy="8" r="1.5"/>
  <circle cx="3" cy="13" r="1.5"/>
  <circle cx="7" cy="13" r="1.5"/>
</svg>`;

const PLUS_SVG = `<svg width="14" height="14" viewBox="0 0 14 14" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" xmlns="http://www.w3.org/2000/svg">
  <line x1="7" y1="2" x2="7" y2="12"/>
  <line x1="2" y1="7" x2="12" y2="7"/>
</svg>`;

/* ------------------------------------------------------------------ */
/*  Plugin View                                                       */
/* ------------------------------------------------------------------ */

class TableControlsView {
  private container: HTMLDivElement;
  private view: EditorView;
  private tableStart = -1;
  private tableNode: PmNode | null = null;
  private tableDom: HTMLElement | null = null;
  private activeRow = -1;
  private activeCol = -1;
  private rowGrips: HTMLButtonElement[] = [];
  private colGrips: HTMLButtonElement[] = [];
  private addRowBtn: HTMLButtonElement | null = null;
  private addColBtn: HTMLButtonElement | null = null;
  private popover: HTMLDivElement | null = null;
  private popoverType: "row" | "col" | null = null;
  private popoverIndex = -1;
  private resizeObserver: ResizeObserver | null = null;
  private boundClosePopover = (e: MouseEvent) => {
    if (this.popover && !this.popover.contains(e.target as Node)) {
      this.closePopover();
    }
  };

  constructor(view: EditorView) {
    this.view = view;
    this.container = document.createElement("div");
    this.container.className = "table-controls-container";
    const editorParent = view.dom.parentElement;
    if (editorParent) {
      editorParent.style.position = "relative";
      editorParent.appendChild(this.container);
    }
    this.resizeObserver = new ResizeObserver(() => {
      if (this.tableDom) this.reposition();
    });
  }

  update(view: EditorView) {
    this.view = view;
    const { state } = view;

    const table = findTable(state);
    if (!table) {
      this.hide();
      return;
    }

    this.tableNode = table.node;
    this.tableStart = table.start;

    // Find DOM element for the table
    const domAtPos = view.domAtPos(table.start + 1);
    let dom = domAtPos.node as HTMLElement;
    while (dom && dom.nodeName !== "TABLE") {
      dom = dom.parentElement as HTMLElement;
    }
    if (!dom) {
      this.hide();
      return;
    }

    if (this.tableDom !== dom) {
      if (this.tableDom) this.resizeObserver?.unobserve(this.tableDom);
      this.tableDom = dom;
      this.resizeObserver?.observe(dom);
    }

    const { row, col } = getCellIndex(state, table.start, table.node);
    this.activeRow = row;
    this.activeCol = col;

    this.render();
    this.reposition();
  }

  destroy() {
    this.resizeObserver?.disconnect();
    document.removeEventListener("mousedown", this.boundClosePopover);
    this.container.remove();
  }

  /* ---- Render ---- */

  private hide() {
    this.container.style.display = "none";
    this.tableNode = null;
    this.tableDom = null;
    this.tableStart = -1;
    this.rowGrips = [];
    this.colGrips = [];
    this.addRowBtn = null;
    this.addColBtn = null;
    this.closePopover();
  }

  private render() {
    if (!this.tableNode || !this.tableDom) return;
    this.container.style.display = "block";

    const map = TableMap.get(this.tableNode);
    const needsRebuild =
      this.rowGrips.length !== map.height ||
      this.colGrips.length !== map.width;

    if (needsRebuild) {
      this.container.innerHTML = "";
      this.rowGrips = [];
      this.colGrips = [];

      for (let r = 0; r < map.height; r++) {
        const btn = document.createElement("button");
        btn.className = "table-grip table-grip-row";
        btn.type = "button";
        btn.innerHTML = GRIP_DOTS_SVG;
        btn.title = `Row ${r + 1}`;
        btn.addEventListener("mousedown", (e) => {
          e.preventDefault();
          e.stopPropagation();
          this.togglePopover("row", r, btn);
        });
        this.container.appendChild(btn);
        this.rowGrips.push(btn);
      }

      for (let c = 0; c < map.width; c++) {
        const btn = document.createElement("button");
        btn.className = "table-grip table-grip-col";
        btn.type = "button";
        btn.innerHTML = GRIP_DOTS_SVG;
        btn.title = `Column ${c + 1}`;
        btn.addEventListener("mousedown", (e) => {
          e.preventDefault();
          e.stopPropagation();
          this.togglePopover("col", c, btn);
        });
        this.container.appendChild(btn);
        this.colGrips.push(btn);
      }

      this.addRowBtn = document.createElement("button");
      this.addRowBtn.className = "table-add-btn table-add-row";
      this.addRowBtn.type = "button";
      this.addRowBtn.innerHTML = PLUS_SVG;
      this.addRowBtn.title = "Add row";
      this.addRowBtn.addEventListener("mousedown", (e) => {
        e.preventDefault();
        e.stopPropagation();
        this.addRow();
      });
      this.container.appendChild(this.addRowBtn);

      this.addColBtn = document.createElement("button");
      this.addColBtn.className = "table-add-btn table-add-col";
      this.addColBtn.type = "button";
      this.addColBtn.innerHTML = PLUS_SVG;
      this.addColBtn.title = "Add column";
      this.addColBtn.addEventListener("mousedown", (e) => {
        e.preventDefault();
        e.stopPropagation();
        this.addColumn();
      });
      this.container.appendChild(this.addColBtn);
    }

    this.rowGrips.forEach((btn, i) => {
      btn.classList.toggle("active", i === this.activeRow);
    });
    this.colGrips.forEach((btn, i) => {
      btn.classList.toggle("active", i === this.activeCol);
    });
  }

  /* ---- Positioning ---- */

  private reposition() {
    if (!this.tableDom || !this.tableNode) return;

    const containerParent = this.container.parentElement;
    if (!containerParent) return;
    const parentRect = containerParent.getBoundingClientRect();
    const tableRect = this.tableDom.getBoundingClientRect();

    const rows = this.tableDom.querySelectorAll("tr");
    rows.forEach((tr, i) => {
      if (i >= this.rowGrips.length) return;
      const grip = this.rowGrips[i];
      const trRect = tr.getBoundingClientRect();
      grip.style.position = "absolute";
      grip.style.left = `${tableRect.left - parentRect.left - 28}px`;
      grip.style.top = `${trRect.top - parentRect.top + trRect.height / 2 - 12}px`;
    });

    const firstRow = rows[0];
    if (firstRow) {
      const cells = firstRow.querySelectorAll("th, td");
      cells.forEach((cell, i) => {
        if (i >= this.colGrips.length) return;
        const grip = this.colGrips[i];
        const cellRect = cell.getBoundingClientRect();
        grip.style.position = "absolute";
        grip.style.left = `${cellRect.left - parentRect.left + cellRect.width / 2 - 12}px`;
        grip.style.top = `${tableRect.top - parentRect.top - 28}px`;
      });
    }

    if (this.addRowBtn) {
      this.addRowBtn.style.position = "absolute";
      this.addRowBtn.style.left = `${tableRect.left - parentRect.left + tableRect.width / 2 - 12}px`;
      this.addRowBtn.style.top = `${tableRect.bottom - parentRect.top + 4}px`;
    }

    if (this.addColBtn) {
      this.addColBtn.style.position = "absolute";
      this.addColBtn.style.left = `${tableRect.right - parentRect.left + 4}px`;
      this.addColBtn.style.top = `${tableRect.top - parentRect.top + tableRect.height / 2 - 12}px`;
    }
  }

  /* ---- Popovers ---- */

  private togglePopover(
    type: "row" | "col",
    index: number,
    anchor: HTMLButtonElement,
  ) {
    if (
      this.popover &&
      this.popoverType === type &&
      this.popoverIndex === index
    ) {
      this.closePopover();
      return;
    }
    this.closePopover();
    this.popoverType = type;
    this.popoverIndex = index;

    const popover = document.createElement("div");
    popover.className = "table-grip-popover";

    if (type === "row") {
      this.buildRowPopover(popover, index);
    } else {
      this.buildColPopover(popover, index);
    }

    const anchorRect = anchor.getBoundingClientRect();
    const containerParent = this.container.parentElement;
    if (!containerParent) return;
    const parentRect = containerParent.getBoundingClientRect();

    popover.style.position = "absolute";
    popover.style.left = `${anchorRect.left - parentRect.left - 4}px`;
    popover.style.top = `${anchorRect.bottom - parentRect.top + 4}px`;

    this.container.appendChild(popover);
    this.popover = popover;

    setTimeout(() => {
      document.addEventListener("mousedown", this.boundClosePopover);
    }, 0);
  }

  private buildRowPopover(el: HTMLDivElement, row: number) {
    if (!this.tableNode) return;
    const map = TableMap.get(this.tableNode);

    const items: PopoverItem[] = [
      {
        label: "Move up",
        disabled: row === 0,
        action: () => {
          const cmd = moveTableRow({ from: row, to: row - 1 });
          cmd(this.view.state, this.view.dispatch);
          this.closePopover();
        },
      },
      {
        label: "Move down",
        disabled: row >= map.height - 1,
        action: () => {
          const cmd = moveTableRow({ from: row, to: row + 1 });
          cmd(this.view.state, this.view.dispatch);
          this.closePopover();
        },
      },
      {
        label: "Delete row",
        destructive: true,
        disabled: map.height <= 1,
        action: () => {
          this.selectRow(row);
          deleteRow(this.view.state, this.view.dispatch);
          this.closePopover();
        },
      },
    ];
    this.buildPopoverItems(el, items);
  }

  private buildColPopover(el: HTMLDivElement, col: number) {
    if (!this.tableNode) return;
    const map = TableMap.get(this.tableNode);

    const items: PopoverItem[] = [
      {
        label: "Move left",
        disabled: col === 0,
        action: () => {
          const cmd = moveTableColumn({ from: col, to: col - 1 });
          cmd(this.view.state, this.view.dispatch);
          this.closePopover();
        },
      },
      {
        label: "Move right",
        disabled: col >= map.width - 1,
        action: () => {
          const cmd = moveTableColumn({ from: col, to: col + 1 });
          cmd(this.view.state, this.view.dispatch);
          this.closePopover();
        },
      },
      {
        label: "Delete column",
        destructive: true,
        disabled: map.width <= 1,
        action: () => {
          this.selectCol(col);
          deleteColumn(this.view.state, this.view.dispatch);
          this.closePopover();
        },
      },
    ];
    this.buildPopoverItems(el, items);
  }

  private buildPopoverItems(el: HTMLDivElement, items: PopoverItem[]) {
    for (const item of items) {
      const btn = document.createElement("button");
      btn.type = "button";
      btn.className = "table-grip-popover-item";
      if (item.destructive) btn.classList.add("destructive");
      if (item.disabled) {
        btn.disabled = true;
        btn.classList.add("disabled");
      }
      btn.textContent = item.label;
      btn.addEventListener("mousedown", (e) => {
        e.preventDefault();
        e.stopPropagation();
        if (!item.disabled) item.action();
      });
      el.appendChild(btn);
    }
  }

  private closePopover() {
    document.removeEventListener("mousedown", this.boundClosePopover);
    if (this.popover) {
      this.popover.remove();
      this.popover = null;
    }
    this.popoverType = null;
    this.popoverIndex = -1;
  }

  /* ---- Actions ---- */

  /** Select an entire row via CellSelection so deleteRow targets it. */
  private selectRow(row: number) {
    if (!this.tableNode) return;
    const map = TableMap.get(this.tableNode);
    const start = this.tableStart + 1;
    const anchorCellPos = start + map.map[row * map.width];
    const headCellPos = start + map.map[row * map.width + map.width - 1];
    const $anchor = this.view.state.doc.resolve(anchorCellPos);
    const $head = this.view.state.doc.resolve(headCellPos);
    const sel = new CellSelection($anchor, $head);
    this.view.dispatch(this.view.state.tr.setSelection(sel));
  }

  /** Select an entire column via CellSelection so deleteColumn targets it. */
  private selectCol(col: number) {
    if (!this.tableNode) return;
    const map = TableMap.get(this.tableNode);
    const start = this.tableStart + 1;
    const anchorCellPos = start + map.map[col];
    const headCellPos = start + map.map[(map.height - 1) * map.width + col];
    const $anchor = this.view.state.doc.resolve(anchorCellPos);
    const $head = this.view.state.doc.resolve(headCellPos);
    const sel = new CellSelection($anchor, $head);
    this.view.dispatch(this.view.state.tr.setSelection(sel));
  }

  private addRow() {
    if (!this.tableNode) return;
    const map = TableMap.get(this.tableNode);
    const start = this.tableStart + 1;
    const lastRowFirstCell = start + map.map[(map.height - 1) * map.width];
    const $pos = this.view.state.doc.resolve(lastRowFirstCell);
    const tr = this.view.state.tr.setSelection(TextSelection.create(this.view.state.doc, $pos.pos));
    this.view.dispatch(tr);
    addRowAfter(this.view.state, this.view.dispatch);
  }

  private addColumn() {
    if (!this.tableNode) return;
    const map = TableMap.get(this.tableNode);
    const start = this.tableStart + 1;
    const firstRowLastCell = start + map.map[map.width - 1];
    const $pos = this.view.state.doc.resolve(firstRowLastCell);
    const tr = this.view.state.tr.setSelection(TextSelection.create(this.view.state.doc, $pos.pos));
    this.view.dispatch(tr);
    addColumnAfter(this.view.state, this.view.dispatch);
  }
}

interface PopoverItem {
  label: string;
  disabled?: boolean;
  destructive?: boolean;
  action: () => void;
}

/* ------------------------------------------------------------------ */
/*  Extension                                                         */
/* ------------------------------------------------------------------ */

export const TableControls = Extension.create({
  name: "tableControls",

  addProseMirrorPlugins() {
    return [
      new Plugin({
        key: TABLE_CONTROLS_KEY,
        view(editorView) {
          return new TableControlsView(editorView);
        },
      }),
    ];
  },
});
