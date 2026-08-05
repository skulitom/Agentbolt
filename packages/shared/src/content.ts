/**
 * Helpers for rendering content blocks. Kept in shared because both the server
 * (asset extraction, thumbnails) and the UI (rendering) need the same view of
 * what a block contains.
 */
export interface DiffLine {
  kind: "add" | "del" | "context" | "meta" | "hunk";
  text: string;
  /** Line numbers in the old/new file, when derivable. */
  oldNo?: number;
  newNo?: number;
}

/**
 * Minimal unified-diff parser. Deliberately dependency-free and forgiving: an
 * agent may hand us `git diff` output, a bare hunk, or something ragged, and a
 * diff that renders slightly imperfectly is far better than a crash.
 */
export function parseUnifiedDiff(text: string): DiffLine[] {
  const out: DiffLine[] = [];
  let oldNo = 0;
  let newNo = 0;

  for (const raw of text.split(/\r?\n/)) {
    if (raw.startsWith("@@")) {
      const match = /@@\s*-(\d+)(?:,\d+)?\s*\+(\d+)(?:,\d+)?\s*@@/.exec(raw);
      if (match) {
        oldNo = Number(match[1]);
        newNo = Number(match[2]);
      }
      out.push({ kind: "hunk", text: raw });
      continue;
    }
    if (
      raw.startsWith("diff ") ||
      raw.startsWith("index ") ||
      raw.startsWith("--- ") ||
      raw.startsWith("+++ ") ||
      raw.startsWith("new file") ||
      raw.startsWith("deleted file") ||
      raw.startsWith("similarity index") ||
      raw.startsWith("rename ")
    ) {
      out.push({ kind: "meta", text: raw });
      continue;
    }
    if (raw.startsWith("+")) {
      out.push({ kind: "add", text: raw.slice(1), newNo: newNo++ });
      continue;
    }
    if (raw.startsWith("-")) {
      out.push({ kind: "del", text: raw.slice(1), oldNo: oldNo++ });
      continue;
    }
    if (raw.startsWith("\\")) {
      out.push({ kind: "meta", text: raw });
      continue;
    }
    const text2 = raw.startsWith(" ") ? raw.slice(1) : raw;
    out.push({ kind: "context", text: text2, oldNo: oldNo++, newNo: newNo++ });
  }

  // Trailing blank line from the final newline adds noise; drop it.
  while (out.length && out[out.length - 1]?.kind === "context" && out[out.length - 1]?.text === "") out.pop();
  return out;
}

export function diffStats(lines: DiffLine[]): { added: number; removed: number } {
  let added = 0;
  let removed = 0;
  for (const line of lines) {
    if (line.kind === "add") added++;
    else if (line.kind === "del") removed++;
  }
  return { added, removed };
}

const DATA_URI = /^data:image\/(png|jpeg|jpg|gif|webp|svg\+xml);base64,[a-z0-9+/=]+$/i;

export function isSafeImageDataUri(value: string): boolean {
  return DATA_URI.test(value);
}
