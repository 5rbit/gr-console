/**
 * Keystroke-level type filters for DataGrid editing.
 *
 * A filter sees the whole prospective edit buffer (not single characters) and
 * must accept in-progress partial states ("", "-", "1.", "1e-") — otherwise
 * legitimate values become untypeable. Final validity (a lone "-", min/max
 * range, enum membership) stays with commit-time validation / red-flag
 * drafts, which a live filter must not pre-empt mid-typing.
 */

/** Partial integer: optional sign, digits. Accepts "" and "-". */
export const INT_PARTIAL = /^-?\d*$/

/** Partial float incl. scientific notation. Accepts "", "-", "1.", ".5", "1e-". */
export const FLOAT_PARTIAL = /^[+-]?(\d+(\.\d*)?|\.\d*)?([eE][+-]?\d*)?$/

export const intFilter = (s: string): boolean => INT_PARTIAL.test(s)
export const floatFilter = (s: string): boolean => FLOAT_PARTIAL.test(s)
