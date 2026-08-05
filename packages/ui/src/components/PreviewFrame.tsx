import { useEffect, useRef, useState } from "react";
import { PreviewToParent, type ParentToPreview } from "@agentui/shared";
import { useSession } from "../store.js";

/**
 * The only way the parent talks to the frame.
 *
 * `ParentToPreview` cannot be validated on arrival — the frame has no bundler
 * and so no zod — which makes this funnel the one place the shape can be
 * checked at all. Posting an object literal directly would skip it silently.
 *
 * targetOrigin is "*" because the frame's origin is discovered at runtime (the
 * shell's port + 1) and nothing here is a secret: the frame already receives
 * the full spec and values by design.
 */
function postToPreview(frame: HTMLIFrameElement | null, message: ParentToPreview): void {
  frame?.contentWindow?.postMessage(message, "*");
}

/**
 * Hosts the agent-authored preview module.
 *
 * The frame lives on a *different* loopback origin (the shell's port + 1). That
 * is what lets `sandbox="allow-scripts allow-same-origin"` be both safe and
 * workable: cross-origin, the frame cannot reach `frameElement` to strip its own
 * sandbox, while being same-origin *to itself* is what makes `import()` and
 * import maps function at all.
 */
export function PreviewFrame({ sessionId, previewOrigin }: { sessionId: string; previewOrigin: string }) {
  const iframeRef = useRef<HTMLIFrameElement>(null);
  const spec = useSession((s) => s.spec);
  const values = useSession((s) => s.values);
  const revision = useSession((s) => s.revision);
  const setPreviewError = useSession((s) => s.setPreviewError);
  const previewError = useSession((s) => s.previewError);
  const transport = useSession((s) => s.transport);
  const [ready, setReady] = useState(false);

  const height = spec?.preview?.height ?? 320;

  // Values change every frame during a drag; push them straight through so the
  // preview is live at 60fps. This is tier 1 of the three-tier debounce and is
  // deliberately NOT throttled — nothing leaves the browser here.
  //
  // `source` rides along so the frame can animate agent pushes but track user
  // drags 1:1. Read via getState() at post time rather than subscribed: the
  // store writes it in the same set() as `values`, so it is always the source
  // of exactly this values object, and subscribing would only add a render.
  useEffect(() => {
    if (!ready) return;
    const source = useSession.getState().valuesSource;
    postToPreview(iframeRef.current, { type: "values", values, source });
  }, [values, ready]);

  useEffect(() => {
    function onMessage(event: MessageEvent) {
      if (event.source !== iframeRef.current?.contentWindow) return;

      // Shape-validated rather than origin-trusted: the frame is hostile by
      // assumption, since an agent wrote what runs inside it.
      const parsed = PreviewToParent.safeParse(event.data);
      if (!parsed.success) return;
      const message = parsed.data;

      if (message.type === "ready-for-init") {
        // Genuinely read fresh, which is what the effect's dep list assumes:
        // the frame asks for init whenever it feels like it, and a drag between
        // mount and that request would otherwise send it values it has already
        // been superseded by.
        const { values: current } = useSession.getState();
        postToPreview(iframeRef.current, { type: "init", sessionId, spec, values: current });
        setReady(true);
      } else if (message.type === "preview_error") {
        setPreviewError({ message: message.message, stack: message.stack });
        // Into the event log too, so the agent sees the stack trace it caused.
        transport?.send({ type: "preview_error", message: message.message, stack: message.stack });
      } else if (message.type === "preview_event") {
        transport?.send({ type: "preview_event", name: message.name, data: message.data });
      } else if (message.type === "ready") {
        setPreviewError(undefined);
      }
    }

    window.addEventListener("message", onMessage);
    return () => window.removeEventListener("message", onMessage);
  }, [sessionId, spec, setPreviewError, transport]);

  // A new revision means the module may have been rewritten; remount the frame
  // so the fresh module loads without a page reload.
  useEffect(() => setReady(false), [revision]);

  return (
    <div className="preview">
      {/* Say what this is. Unlabelled, a bordered box reads as content rather
          than as a rendering of your own choices. */}
      <div className="preview-head">
        <span className="preview-title">Preview</span>
        <span className="preview-hint">{ready ? "updates as you adjust" : "loading…"}</span>
      </div>
      <iframe
        key={revision}
        ref={iframeRef}
        title="Live preview"
        src={`${previewOrigin}/preview.html?s=${encodeURIComponent(sessionId)}&r=${revision}`}
        sandbox="allow-scripts allow-same-origin"
        style={{ height }}
        data-ready={ready}
      />
      {previewError ? (
        <div className="preview-error">
          <strong>Preview error</strong> — reported to the agent.
          <pre>{previewError.message}</pre>
        </div>
      ) : null}
    </div>
  );
}
