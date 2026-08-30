import { describe, it, expect } from 'vitest';
import { resolve_theme, is_theme_preference } from './theme.svelte';

describe('resolve_theme', () => {
    it('honours an explicit choice regardless of the system', () => {
        expect(resolve_theme('dark', false)).toBe(true);
        expect(resolve_theme('dark', true)).toBe(true);
        expect(resolve_theme('light', true)).toBe(false);
        expect(resolve_theme('light', false)).toBe(false);
    });

    it('follows the system when no explicit choice is made', () => {
        expect(resolve_theme('system', true)).toBe(true);
        expect(resolve_theme('system', false)).toBe(false);
    });
});

describe('is_theme_preference', () => {
    it('accepts the three valid values', () => {
        expect(is_theme_preference('system')).toBe(true);
        expect(is_theme_preference('light')).toBe(true);
        expect(is_theme_preference('dark')).toBe(true);
    });

    it('rejects anything else, so stored rubbish falls back to the system', () => {
        for (const bad of [null, undefined, '', 'Dark', 'auto', 0, {}]) {
            expect(is_theme_preference(bad), String(bad)).toBe(false);
        }
    });
});
