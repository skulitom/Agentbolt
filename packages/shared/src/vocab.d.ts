/**
 * Types for `vocab.js`, which is hand-written JavaScript so the preview origin
 * can import it verbatim (see the header there). Kept as a separate .d.ts
 * rather than turning on `allowJs`, so the one JS file in `src/` stays an
 * explicit exception rather than a mode the whole package runs in.
 *
 * The predicates take a structural shape rather than `Control`: the same
 * functions run in the frame against a spec that arrived over postMessage and
 * was never parsed, so they must not assume a validated discriminated union.
 */

/** The parts of a control these predicates actually read. */
export interface VocabControl {
  type: string;
  id: string;
  label?: string | undefined;
  unit?: string | undefined;
  options?: ReadonlyArray<{ value: string }> | undefined;
}

export declare const SIZE_HINTS: RegExp;
export declare const FONT_HINTS: RegExp;
export declare const DURATION_HINT: RegExp;
export declare const DELAY_HINT: RegExp;
export declare const EASING_NAME_HINT: RegExp;
export declare const EASING_OPTION: RegExp;

export declare function isEasingSelect(control: VocabControl): boolean;
export declare function isDurationish(control: VocabControl): boolean;
export declare function isSizeish(control: VocabControl): boolean;
export declare function isFontish(control: VocabControl): boolean;

export declare const SELF_SHOWN: readonly string[];

export declare function roundForDisplay(value: unknown): string;
