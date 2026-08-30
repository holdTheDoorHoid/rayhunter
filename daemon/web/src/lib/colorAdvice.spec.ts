import { describe, it, expect } from 'vitest';
import {
    parse_hex,
    brightness,
    is_too_dark,
    are_too_similar,
    similar_threat_pairs,
    distance,
} from './colorAdvice';

describe('parse_hex', () => {
    it('accepts six hex digits with or without a leading hash', () => {
        expect(parse_hex('#ff8000')).toEqual({ r: 255, g: 128, b: 0 });
        expect(parse_hex('00FF00')).toEqual({ r: 0, g: 255, b: 0 });
    });

    it('rejects anything else', () => {
        expect(parse_hex('#fff')).toBeNull();
        expect(parse_hex('#gggggg')).toBeNull();
        expect(parse_hex('')).toBeNull();
    });
});

describe('is_too_dark', () => {
    it('flags colors that would vanish against the black screen', () => {
        expect(is_too_dark('#000000')).toBe(true);
        expect(is_too_dark('#101010')).toBe(true);
        expect(is_too_dark('#002000')).toBe(true);
    });

    it('does not flag saturated colors, however dark they look numerically', () => {
        // Pure blue is Rayhunter's colorblind recording color. A WCAG contrast
        // check would fail it against black; as a solid bar it is clearly visible.
        expect(is_too_dark('#0000ff')).toBe(false);
        expect(brightness({ r: 0, g: 0, b: 255 })).toBe(1);
    });

    it('does not flag any of the built-in defaults', () => {
        for (const hex of ['#ffffff', '#00ff00', '#0000ff', '#ffff00', '#ffa500', '#ff0000']) {
            expect(is_too_dark(hex), `${hex} should not be flagged`).toBe(false);
        }
    });

    it('treats an unparseable color as not dark, leaving it to the daemon', () => {
        expect(is_too_dark('nonsense')).toBe(false);
    });
});

describe('are_too_similar', () => {
    it('flags near-identical colors', () => {
        expect(are_too_similar('#ff0000', '#ff1010')).toBe(true);
        expect(are_too_similar('#123456', '#123456')).toBe(true);
    });

    it('does not flag the built-in warning colors against each other', () => {
        expect(are_too_similar('#ffff00', '#ffa500')).toBe(false); // low vs medium
        expect(are_too_similar('#ffa500', '#ff0000')).toBe(false); // medium vs high
        expect(are_too_similar('#ffff00', '#ff0000')).toBe(false); // low vs high
    });

    it('is symmetric', () => {
        expect(distance({ r: 10, g: 20, b: 30 }, { r: 200, g: 100, b: 50 })).toBeCloseTo(
            distance({ r: 200, g: 100, b: 50 }, { r: 10, g: 20, b: 30 })
        );
    });
});

describe('similar_threat_pairs', () => {
    const defaults = [
        { key: 'paused', label: 'Paused', hex: '#ffffff' },
        { key: 'recording', label: 'Recording', hex: '#00ff00' },
        { key: 'warning_low', label: 'Low', hex: '#ffff00' },
        { key: 'warning_medium', label: 'Medium', hex: '#ffa500' },
        { key: 'warning_high', label: 'High', hex: '#ff0000' },
    ];

    it('reports nothing for the built-in colors', () => {
        expect(similar_threat_pairs(defaults)).toEqual([]);
    });

    it('reports severity levels a user has made indistinguishable', () => {
        const colors = defaults.map((c) =>
            c.key === 'warning_high' ? { ...c, hex: '#ffa000' } : c
        );
        expect(similar_threat_pairs(colors)).toEqual([['Medium', 'High']]);
    });

    it('ignores non-severity states, which are not a safety problem', () => {
        // Paused and recording identical: confusing, but not a missed warning.
        const colors = defaults.map((c) => (c.key === 'recording' ? { ...c, hex: '#ffffff' } : c));
        expect(similar_threat_pairs(colors)).toEqual([]);
    });

    it('reports every clashing pair when all three are alike', () => {
        const colors = defaults.map((c) =>
            c.key.startsWith('warning_') ? { ...c, hex: '#ff0000' } : c
        );
        expect(similar_threat_pairs(colors)).toHaveLength(3);
    });
});
