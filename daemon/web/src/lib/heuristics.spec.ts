import { describe, it, expect } from 'vitest';
import { HEURISTICS, HEURISTICS_BY_KEY, type AnalyzerKey } from './heuristics';

/**
 * Every analyzer the daemon exposes must have an explanation. The settings page
 * is where someone decides to switch a detector off, and that is not a safe
 * decision to make from the setting's name alone.
 */
const EXPECTED_ANALYZERS: AnalyzerKey[] = [
    'imsi_requested',
    'connection_redirect_2g_downgrade',
    'lte_sib6_and_7_downgrade',
    'null_cipher',
    'nas_null_cipher',
    'incomplete_sib',
    'lpp_location_request',
    'lpp_location_tracking',
    'rrlp_location_request',
    'diagnostic_analyzer',
    'test_analyzer',
];

describe('heuristic explanations', () => {
    it('covers every analyzer, with none left over', () => {
        expect(HEURISTICS.map((h) => h.key).sort()).toEqual([...EXPECTED_ANALYZERS].sort());
    });

    it('has no duplicate entries', () => {
        const keys = HEURISTICS.map((h) => h.key);
        expect(new Set(keys).size).toBe(keys.length);
    });

    it('gives each analyzer a title, summary, and both explanation halves', () => {
        for (const h of HEURISTICS) {
            expect(h.title.length, `${h.key} title`).toBeGreaterThan(0);
            expect(h.summary.length, `${h.key} summary`).toBeGreaterThan(0);
            expect(h.detects.length, `${h.key} detects`).toBeGreaterThan(0);
            expect(h.matters.length, `${h.key} matters`).toBeGreaterThan(0);
        }
    });

    it('keeps the always visible summary short enough to read at a glance', () => {
        for (const h of HEURISTICS) {
            expect(
                h.summary.length,
                `${h.key} summary is too long to sit under a checkbox`
            ).toBeLessThanOrEqual(160);
        }
    });

    it('avoids dashes as punctuation in text people read', () => {
        for (const h of HEURISTICS) {
            const prose = [h.title, h.summary, h.detects, h.matters, h.noise ?? ''].join(' ');
            expect(prose, `${h.key} contains a dash used as punctuation`).not.toMatch(/[—–]| - /);
        }
    });

    it('is reachable by key', () => {
        expect(HEURISTICS_BY_KEY.null_cipher.title).toBe('Encryption switched off by the tower');
    });
});
