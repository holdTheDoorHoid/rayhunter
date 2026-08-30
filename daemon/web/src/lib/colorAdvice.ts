/**
 * Checks that warn about color choices which would make the device harder to
 * read, or actively misleading.
 *
 * Rayhunter is a warning device, so the cost of a bad color choice is not
 * cosmetic: a status line nobody can see, or two threat levels nobody can tell
 * apart, means a real alert gets missed. These run in the browser as advice —
 * nothing here blocks a choice, since a user may have a reason we can't see.
 */

export interface Rgb {
    r: number;
    g: number;
    b: number;
}

export function parse_hex(hex: string): Rgb | null {
    const body = hex.startsWith('#') ? hex.slice(1) : hex;
    if (!/^[0-9a-fA-F]{6}$/.test(body)) return null;
    return {
        r: parseInt(body.slice(0, 2), 16),
        g: parseInt(body.slice(2, 4), 16),
        b: parseInt(body.slice(4, 6), 16),
    };
}

/**
 * How bright the brightest channel is, 0..1.
 *
 * This is deliberately not WCAG relative luminance. That models text contrast
 * and would rate a saturated blue as failing against black — yet blue is
 * Rayhunter's own colorblind recording color and is perfectly visible as a
 * block of color. What actually makes a solid bar invisible on this device's
 * black screen is every channel being low, which is what this measures.
 */
export function brightness(color: Rgb): number {
    return Math.max(color.r, color.g, color.b) / 255;
}

/** Below this, a color reads as near-black on the device's black background. */
export const DIM_THRESHOLD = 0.25;

export function is_too_dark(hex: string): boolean {
    const c = parse_hex(hex);
    return c === null ? false : brightness(c) < DIM_THRESHOLD;
}

/**
 * Perceptual-ish distance between two colors, using the "redmean" weighting.
 * Cheap, and much closer to human perception than plain RGB distance — good
 * enough for "could someone confuse these two at a glance on a small screen".
 * Range is roughly 0..765.
 */
export function distance(a: Rgb, b: Rgb): number {
    const redmean = (a.r + b.r) / 2;
    const dr = a.r - b.r;
    const dg = a.g - b.g;
    const db = a.b - b.b;
    return Math.sqrt(
        (2 + redmean / 256) * dr * dr + 4 * dg * dg + (2 + (255 - redmean) / 256) * db * db
    );
}

/**
 * Below this, two colors are close enough that a glance at a thin line on a
 * 128px screen may not separate them. Rayhunter's own defaults (yellow/orange/
 * red) sit at 180 and above, so they clear this comfortably.
 */
export const SIMILAR_THRESHOLD = 100;

export function are_too_similar(a: string, b: string): boolean {
    const ca = parse_hex(a);
    const cb = parse_hex(b);
    if (ca === null || cb === null) return false;
    return distance(ca, cb) < SIMILAR_THRESHOLD;
}

export interface LabelledColor {
    key: string;
    label: string;
    hex: string;
}

/**
 * Pairs of threat levels that are too alike to tell apart. Only severity levels
 * are compared: confusing "paused" with "recording" is a nuisance, but confusing
 * a low warning with a high one is the failure that matters.
 */
export function similar_threat_pairs(colors: LabelledColor[]): [string, string][] {
    const severities = colors.filter((c) => c.key.startsWith('warning_'));
    const pairs: [string, string][] = [];
    for (let i = 0; i < severities.length; i++) {
        for (let j = i + 1; j < severities.length; j++) {
            if (are_too_similar(severities[i].hex, severities[j].hex)) {
                pairs.push([severities[i].label, severities[j].label]);
            }
        }
    }
    return pairs;
}
