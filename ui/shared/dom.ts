// All dynamic values go through textContent: agent strings are untrusted.
export function el<K extends keyof HTMLElementTagNameMap>(
  tag: K, text?: string, attrs: Record<string, string> = {}, ...children: (Node | string)[]
): HTMLElementTagNameMap[K] {
  const n = document.createElement(tag);
  if (text !== undefined) n.textContent = text;
  for (const [k, v] of Object.entries(attrs)) n.setAttribute(k, v);
  n.append(...children);
  return n;
}
