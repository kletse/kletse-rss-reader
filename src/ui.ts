// Small DOM helpers shared by the views.
export const $ = <T extends HTMLElement = HTMLElement>(id: string) => document.getElementById(id) as T;

let toastTimer: number | undefined;
export function toast(message: string, kind: "info" | "error" = "info") {
  const el = $("toast");
  el.textContent = message;
  el.className = kind;
  el.hidden = false;
  clearTimeout(toastTimer);
  toastTimer = window.setTimeout(() => (el.hidden = true), kind === "error" ? 6000 : 2200);
}

export function el<K extends keyof HTMLElementTagNameMap>(
  tag: K, attrs: Record<string, string> = {}, ...children: (Node | string)[]
): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs)) node.setAttribute(k, v);
  node.append(...children);
  return node;
}

/** Shows a yes/no overlay; resolves true on Enter/y. */
export function confirmDialog(text: string): Promise<boolean> {
  const box = $("confirm");
  $("confirm-text").textContent = text;
  box.hidden = false;
  return new Promise((resolve) => {
    const onKey = (e: KeyboardEvent) => {
      const yes = e.key === "Enter" || e.key === "y";
      const no = e.key === "Escape" || e.key === "n";
      if (!yes && !no) return;
      e.preventDefault();
      e.stopImmediatePropagation();
      window.removeEventListener("keydown", onKey, true);
      box.hidden = true;
      resolve(yes);
    };
    window.addEventListener("keydown", onKey, true);
  });
}
