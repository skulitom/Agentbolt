import { useMemo } from "react";
import DOMPurify from "dompurify";
import { marked } from "marked";
import { diffStats, parseUnifiedDiff, type ContentBlock } from "@agentui/shared";

/**
 * Read-only content an agent attached: prose, code, a diff, a screenshot.
 *
 * Markdown is agent-authored, so it is rendered then sanitised — `marked` is a
 * formatter, not a security boundary. Raw HTML is stripped entirely; a spec that
 * genuinely needs HTML uses the `html` block, which renders in the sandboxed
 * preview frame instead of here.
 */
export function ContentRenderer({ blocks }: { blocks: ContentBlock[] }) {
  if (!blocks.length) return null;
  return (
    <div className="content">
      {blocks.map((block, index) => (
        <Block key={index} block={block} />
      ))}
    </div>
  );
}

function Block({ block }: { block: ContentBlock }) {
  switch (block.kind) {
    case "markdown":
      return <Markdown value={block.value} />;
    case "code":
      return (
        <pre className="code-block" data-language={block.language}>
          <code>{block.value}</code>
        </pre>
      );
    case "diff":
      return <Diff value={block.value} filename={block.filename} />;
    case "image":
      return <ImageBlock block={block} />;
    case "html":
      // Untrusted: rendered in the sandboxed frame (wired in P4), never inline.
      return <div className="notice">HTML content renders in the preview frame.</div>;
  }
}

function Markdown({ value }: { value: string }) {
  const html = useMemo(() => {
    const rendered = marked.parse(value, { async: false, gfm: true, breaks: true });
    return DOMPurify.sanitize(String(rendered), {
      ALLOWED_TAGS: [
        "p", "br", "strong", "em", "del", "code", "pre", "blockquote",
        "ul", "ol", "li", "h1", "h2", "h3", "h4", "h5", "h6",
        "a", "hr", "table", "thead", "tbody", "tr", "th", "td",
      ],
      ALLOWED_ATTR: ["href", "title"],
      // Defence in depth: no javascript:/data: destinations.
      ALLOWED_URI_REGEXP: /^(?:https?:|mailto:|#|\/)/i,
    });
  }, [value]);

  return <div className="markdown" dangerouslySetInnerHTML={{ __html: html }} />;
}

function Diff({ value, filename }: { value: string; filename?: string | undefined }) {
  const lines = useMemo(() => parseUnifiedDiff(value), [value]);
  const stats = useMemo(() => diffStats(lines), [lines]);

  return (
    <div className="diff">
      <div className="diff-head">
        <span className="diff-file">{filename ?? "diff"}</span>
        <span className="diff-stats">
          <span className="added">+{stats.added}</span> <span className="removed">-{stats.removed}</span>
        </span>
      </div>
      <pre className="diff-body">
        {lines.map((line, index) => (
          <div key={index} className={`diff-line diff-${line.kind}`}>
            <span className="gutter">{line.kind === "add" ? "+" : line.kind === "del" ? "-" : " "}</span>
            <span className="text">{line.text || " "}</span>
          </div>
        ))}
      </pre>
    </div>
  );
}

function ImageBlock({ block }: { block: Extract<ContentBlock, { kind: "image" }> }) {
  const src = block.path ?? block.dataUri;
  if (!src) return null;
  return (
    <figure className="image-block">
      <img src={src} alt={block.alt ?? ""} />
      {block.caption ? <figcaption>{block.caption}</figcaption> : null}
    </figure>
  );
}
