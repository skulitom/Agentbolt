import { useCallback, useEffect, useState } from "react";
import type { WidgetSummary } from "@agentui/shared";

/**
 * The human side of the widget library.
 *
 * The agent has its own discovery path (a ranked shortlist in the tool
 * description, plus search). This is yours: when you know a widget exists and
 * the agent has not thought to reuse it, pinning it forces it into that
 * shortlist — a one-click fix for "why did it rebuild that?".
 *
 * Opening one here starts a real session with no agent involved, which still
 * writes output.json, so you can point an agent at the result afterwards.
 */
export function LibraryView() {
  const [query, setQuery] = useState("");
  const [rows, setRows] = useState<WidgetSummary[]>([]);
  const [total, setTotal] = useState(0);
  const [busy, setBusy] = useState(false);

  const load = useCallback(async (q: string) => {
    const response = await fetch(`/api/library?q=${encodeURIComponent(q)}`);
    const body = (await response.json()) as { results: WidgetSummary[]; total: number };
    setRows(body.results);
    setTotal(body.total);
  }, []);

  useEffect(() => {
    const timer = window.setTimeout(() => void load(query), 150);
    return () => window.clearTimeout(timer);
  }, [query, load]);

  useEffect(() => {
    document.title = "Widget library — AgentUI";
  }, []);

  async function open(slug: string) {
    setBusy(true);
    try {
      const response = await fetch(`/api/library/${encodeURIComponent(slug)}/open`, { method: "POST" });
      const body = (await response.json()) as { url?: string };
      if (body.url) window.location.href = body.url;
    } finally {
      setBusy(false);
    }
  }

  async function togglePin(row: WidgetSummary) {
    await fetch(`/api/library/${encodeURIComponent(row.slug)}/pin`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ pinned: !row.pinned }),
    });
    void load(query);
  }

  async function remove(slug: string) {
    if (!window.confirm(`Delete the saved widget "${slug}"? This cannot be undone.`)) return;
    await fetch(`/api/library/${encodeURIComponent(slug)}`, { method: "DELETE" });
    void load(query);
  }

  return (
    <div className="library">
      <header className="session-head">
        <div>
          <h1>Widget library</h1>
          <p className="muted">
            {total ? `${total} saved` : "Nothing saved yet"} · <a href="/">back to widgets</a>
          </p>
        </div>
      </header>

      <input
        type="search"
        placeholder="Search by name, tag or what it does"
        value={query}
        onChange={(e) => setQuery(e.target.value)}
        aria-label="Search saved widgets"
      />

      {rows.length === 0 ? (
        <p className="muted" style={{ marginTop: 16 }}>
          {query ? "Nothing matched." : "When an agent saves a widget worth keeping, it appears here."}
        </p>
      ) : (
        <ul className="library-list">
          {rows.map((row) => (
            <li key={row.slug}>
              <div className="library-row">
                <div className="library-main">
                  <span className="library-name">{row.name}</span>
                  <code className="library-slug">{row.slug}</code>
                  {row.pinned ? <span className="tag-pin">pinned</span> : null}
                  {row.scope === "project" ? <span className="tag-default">project</span> : null}
                  <p className="muted library-desc">{row.description || "—"}</p>
                  <p className="library-meta">
                    {row.uses ? `used ${row.uses}×` : "never used"}
                    {row.hasPreview ? " · live preview" : ""}
                    {row.tags.length ? ` · ${row.tags.join(", ")}` : ""}
                  </p>
                </div>
                <div className="library-actions">
                  <button type="button" className="btn-primary" disabled={busy} onClick={() => void open(row.slug)}>
                    Open
                  </button>
                  <button type="button" className="subtle" onClick={() => void togglePin(row)}
                    title="Pinned widgets are always offered to the agent">
                    {row.pinned ? "Unpin" : "Pin"}
                  </button>
                  <button type="button" className="subtle" onClick={() => void remove(row.slug)}>
                    Delete
                  </button>
                </div>
              </div>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
