import { describe, it, expect } from 'vitest';
import { help_shown_from_stored, HELP_STORAGE_KEY } from './helpVisibility.svelte';

/**
 * The default matters more than it looks. A device that does not explain
 * itself is much harder to learn from, so the help is on unless somebody has
 * deliberately turned it off.
 */
describe('help_shown_from_stored', () => {
    it('is on when nothing has been chosen', () => {
        expect(help_shown_from_stored(null)).toBe(true);
    });

    it('is off only when explicitly turned off', () => {
        expect(help_shown_from_stored('false')).toBe(false);
        expect(help_shown_from_stored('true')).toBe(true);
    });

    /** A value that did not come from here must not silently hide the help. */
    it('treats anything unrecognised as on', () => {
        expect(help_shown_from_stored('rubbish')).toBe(true);
        expect(help_shown_from_stored('')).toBe(true);
        expect(help_shown_from_stored('False')).toBe(true);
    });

    it('uses a key namespaced to this application', () => {
        expect(HELP_STORAGE_KEY).toContain('rayhunter');
    });
});
