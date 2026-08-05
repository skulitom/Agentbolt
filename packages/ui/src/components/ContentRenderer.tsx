import { useMemo, type CSSProperties, type ReactElement } from "react";
import DOMPurify from "dompurify";
import { marked } from "marked";
import {
  diffStats,
  evaluateSample,
  interpolate,
  parseUnifiedDiff,
  type ContentBlock,
  type Control,
  type SampleBlock,
  type Values,
} from "@agentui/shared";
import { useSession } from "../store.js";

/**
 * Read-only content an agent attached: prose, code, a diff, a screenshot, or a
 * rule replayed against real rows.
 *
 * Markdown is agent-authored, so it is rendered then sanitised — `marked` is a
 * formatter, not a security boundary. Raw HTML is stripped entirely: there is
 * no escape hatch for it, because the one that existed (`kind: "html"`) parsed
 * cleanly and then rendered a fixed notice string.
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

/**
 * One renderer per content kind, keyed by the union — same mapped-Record
 * exhaustiveness check as CONTROL_INPUTS, and for the same reason: `ReactElement`
 * rather than `ReactElement | null` means a kind with no renderer is a compile
 * error instead of a block that silently draws nothing.
 */
type BlockRegistry = {
  [K in ContentBlock["kind"]]: (props: { block: Extract<ContentBlock, { kind: K }> }) => ReactElement;
};

const BLOCK_RENDERERS: BlockRegistry = {
  markdown: ({ block }) => (block.bind ? <BoundMarkdown value={block.value} /> : <Markdown value={block.value} />),
  code: ({ block }) => <Code block={block} />,
  diff: ({ block }) => <Diff value={block.value} filename={block.filename} />,
  image: ({ block }) => <ImageBlock block={block} />,
  sample: ({ block }) => <Sample block={block} />,
};

function Block({ block }: { block: ContentBlock }) {
  // The one cast, for the same reason as inputFor: TypeScript cannot correlate
  // a union key with a union of narrowed component types. The registry above is
  // fully checked, which is where a mistake would be made.
  const Renderer = BLOCK_RENDERERS[block.kind] as (props: { block: ContentBlock }) => ReactElement;
  return <Renderer block={block} />;
}

/**
 * Live values, for the blocks that have a reason to want them.
 *
 * Subscribed here rather than threaded down from SessionView so that a drag
 * re-renders the two or three blocks that actually change, not the whole
 * content column. Reads LOCAL values — the same ones the preview reads — so it
 * tracks the finger at 60fps, sends nothing, and cannot wake the agent.
 */
function useLiveValues(): { values: Values; controls: Control[] } {
  const values = useSession((s) => s.values);
  const controls = useSession((s) => s.spec?.controls);
  return { values, controls: controls ?? [] };
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

/**
 * Substitution happens BEFORE marked and before DOMPurify, deliberately: the
 * substituted text is then treated as exactly as untrusted as the rest of the
 * block, and there is no second sanitisation path to get wrong. A control value
 * is user input, and user input that skipped the sanitiser would be the one
 * injection hole in the shell.
 */
function BoundMarkdown({ value }: { value: string }) {
  const { values, controls } = useLiveValues();
  const bound = useMemo(() => interpolate(value, controls, values), [value, controls, values]);
  return <Markdown value={bound} />;
}

function Code({ block }: { block: Extract<ContentBlock, { kind: "code" }> }) {
  const { values, controls } = useLiveValues();
  // Unbound blocks never touch interpolate: a widget about a Helm chart or a
  // GitHub Action must be able to contain braces and get them back verbatim.
  const text = block.bind ? interpolate(block.value, controls, values) : block.value;
  return (
    <div className="code">
      {block.filename ? <div className="code-head">{block.filename}</div> : null}
      <pre className="code-block" data-language={block.language}>
        <code>{text}</code>
      </pre>
    </div>
  );
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
            <span className="text">{line.text || " "}</span>
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

/**
 * The rule, applied to rows the reader recognises.
 *
 * Two modes, from the operator alone. An ORDERING rule sorts and draws one line
 * across the table at the live control value — you drag and watch a row cross
 * it, which is the thing the number alone can never tell you. An EQUALITY rule
 * is a lookup: no sort, the matching row lit up, the agent's own arithmetic
 * already done and its formula printed in `source` where it can be argued with.
 */
function Sample({ block }: { block: SampleBlock }) {
  const { values, controls } = useLiveValues();
  const result = useMemo(() => evaluateSample(block, controls, values), [block, controls, values]);
  if (!result) return null;

  const control = controls.find((c) => c.id === result.controlId);
  const [yes, no] = block.verdict ?? ["matches", "does not"];
  const unit = block.unit ?? "";
  const axis = result.axis;
  // One custom property drives the threshold rule through every track and the
  // caption under them, so the segments line up into a single continuous line.
  const style = axis?.threshold !== undefined ? ({ ["--sample-cut" as string]: `${axis.threshold * 100}%` } as CSSProperties) : undefined;

  return (
    <section className="sample" style={style} data-plotted={Boolean(axis)}>
      <div className="sample-head">
        <span className="sample-label">{block.label}</span>
        {/* Always shown, never optional. A table of numbers with no stated
            provenance is exactly the fabrication this block exists to make
            visible, and a reader who sees "illustrative" discounts correctly. */}
        <span className="sample-source">{block.source}</span>
      </div>

      <ol className="sample-rows">
        {result.rows.map((row, index) => (
          <li key={`${row.label}-${index}`} className="sample-row" data-matched={row.matched}>
            <span className="sample-cell sample-row-label">
              {row.label}
              {row.note ? <span className="sample-note">{row.note}</span> : null}
            </span>
            <span className="sample-cell sample-row-value">
              {row.text}
              {unit ? <span className="unit">{unit}</span> : null}
            </span>
            {axis ? (
              <span className="sample-cell sample-track" data-beyond={row.beyond ?? "no"}>
                <span className="sample-bar" style={{ width: `${(row.fraction ?? 0) * 100}%` }} />
              </span>
            ) : (
              // No honest axis: an equality lookup over strings, or values that
              // are not numbers. Say which row won in words rather than draw a
              // bar whose length means nothing.
              <span className="sample-cell sample-verdict">{row.matched ? yes : ""}</span>
            )}
          </li>
        ))}

        {axis ? (
          // An <li> so the <ol> stays valid HTML, hidden from assistive tech
          // because every number on it is already announced by the rows.
          <li className="sample-axis" aria-hidden="true">
            <span className="sample-axis-end">
              {axis.minText}
              {unit}
            </span>
            {axis.threshold !== undefined ? (
              // Centred on the cut, except near the ends where centring would
              // push the pill out of the plot and over the numbers.
              <span
                className="sample-cut-label"
                data-side={axis.threshold < 0.18 ? "start" : axis.threshold > 0.82 ? "end" : "mid"}
              >
                {control?.label ?? result.controlId} {result.controlText}
                {unit}
              </span>
            ) : null}
            <span className="sample-axis-end">
              {axis.maxText}
              {unit}
              {/* The bound is the answer space, not the data: saying so is what
                  makes a row pinned past the end read as "no setting reaches it". */}
              {axis.fromControl ? "+" : ""}
            </span>
          </li>
        ) : null}
      </ol>

      <div className="sample-foot">
        <span>
          <b>{result.matchCount}</b> of {result.rows.length} {yes}
          {result.matchCount < result.rows.length ? (
            <span className="muted"> · {result.rows.length - result.matchCount} {no}</span>
          ) : null}
        </span>
        {result.total ? (
          <span className="sample-total">
            {result.total.label} <b>{result.total.text}</b>
            {result.total.unit ?? ""}
          </span>
        ) : null}
      </div>
    </section>
  );
}
