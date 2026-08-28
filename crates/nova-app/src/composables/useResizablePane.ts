import { computed, ref, type Ref } from "vue";

export interface ResizablePaneOptions {
  storageKey: string;
  lastExpandedStorageKey: string;
  defaultSize: number;
  /** Collapsed floor. */
  minSize: number;
  /** Derives the max size from the container each drag/clamp — e.g. container size minus the other pane's minimum and the divider's own size. */
  getMax: (containerEl: HTMLElement | null) => number;
  /** Which axis of the drag drives the size delta. */
  axis: "horizontal" | "vertical";
  /** Sign of the delta — which drag direction grows the pane. */
  direction: 1 | -1;
}

/**
 * A draggable, collapsible, persisted pane size — extracted from the
 * response pane's original implementation (the only precedent for this in
 * the codebase) so the sidebar and the request/response split can share one
 * clamp/persist/collapse/drag implementation instead of three copies of it.
 *
 * Clamping only happens on drag and on explicit `setSize` calls (including
 * `toggleCollapsed`) — it does not react to the container resizing on its
 * own (e.g. an OS window resize while nothing is being dragged). That's a
 * pre-existing limitation carried over from the original implementation,
 * not a regression introduced here.
 */
export function useResizablePane(opts: ResizablePaneOptions) {
  const containerEl: Ref<HTMLElement | null> = ref(null);
  const size = ref(Number(localStorage.getItem(opts.storageKey)) || opts.defaultSize);
  const isCollapsed = computed(() => size.value <= opts.minSize);
  let lastExpanded = Number(localStorage.getItem(opts.lastExpandedStorageKey)) || opts.defaultSize;

  function clamp(value: number): number {
    const max = opts.getMax(containerEl.value);
    return Math.min(Math.max(value, opts.minSize), Math.max(max, opts.minSize));
  }

  function setSize(value: number) {
    size.value = clamp(value);
    localStorage.setItem(opts.storageKey, String(size.value));
    if (size.value > opts.minSize) {
      lastExpanded = size.value;
      localStorage.setItem(opts.lastExpandedStorageKey, String(lastExpanded));
    }
  }

  function toggleCollapsed() {
    setSize(isCollapsed.value ? lastExpanded : opts.minSize);
  }

  let dragging = false;

  function startDrag(event: MouseEvent) {
    dragging = true;
    const start = opts.axis === "horizontal" ? event.clientX : event.clientY;
    const startSize = size.value;
    document.body.style.cursor = opts.axis === "horizontal" ? "col-resize" : "row-resize";

    function onMove(moveEvent: MouseEvent) {
      if (!dragging) return;
      const current = opts.axis === "horizontal" ? moveEvent.clientX : moveEvent.clientY;
      const delta = opts.direction * (current - start);
      setSize(startSize + delta);
    }
    function onUp() {
      dragging = false;
      document.body.style.cursor = "";
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
    }
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
    event.preventDefault();
  }

  return { containerEl, size, isCollapsed, setSize, toggleCollapsed, startDrag };
}
