import { describe, it, expect } from 'vitest';
import {
    format_minutes,
    format_interval,
    rotation_summary,
    rotation_warning,
    TIME_PRESETS_MINUTES,
    SIZE_PRESETS_MB,
} from './recordingRotation';

describe('format_minutes', () => {
    it('says minutes below an hour', () => {
        expect(format_minutes(1)).toBe('1 minute');
        expect(format_minutes(15)).toBe('15 minutes');
        expect(format_minutes(59)).toBe('59 minutes');
    });

    it('says whole hours as hours', () => {
        expect(format_minutes(60)).toBe('1 hour');
        expect(format_minutes(120)).toBe('2 hours');
        expect(format_minutes(720)).toBe('12 hours');
    });

    it('says whole days as days', () => {
        expect(format_minutes(1440)).toBe('1 day');
        expect(format_minutes(2880)).toBe('2 days');
    });

    it('spells out an awkward value rather than rounding it away', () => {
        expect(format_minutes(90)).toBe('1 hour 30 minutes');
        expect(format_minutes(61)).toBe('1 hour 1 minute');
    });

    it('has a readable phrase for every preset offered', () => {
        for (const minutes of TIME_PRESETS_MINUTES) {
            expect(format_minutes(minutes)).not.toMatch(/NaN|undefined/);
        }
    });
});

describe('rotation_summary', () => {
    /**
     * The two limits interact, and reading two separate fields does not make
     * the combined behaviour obvious. This sentence is the difference between
     * a setting somebody can predict and one they have to test.
     */
    it('says plainly when nothing will rotate', () => {
        expect(rotation_summary(null, null)).toContain('until you stop it');
        // Zero is how a hand edited config says "off", and must read the same.
        expect(rotation_summary(0, 0)).toContain('until you stop it');
    });

    it('describes a size limit on its own', () => {
        expect(rotation_summary(10, null)).toBe(
            'A new recording starts each time the current one reaches 10 MB.'
        );
    });

    it('describes a time limit on its own', () => {
        expect(rotation_summary(null, 60)).toBe('A new recording starts every hour.');
    });

    it('describes both as whichever comes first', () => {
        expect(rotation_summary(5, 15)).toBe(
            'A new recording starts every 15 minutes, or sooner if the current one reaches 5 MB.'
        );
    });

    it('never leaves a number missing for any preset pairing', () => {
        for (const minutes of TIME_PRESETS_MINUTES) {
            for (const mb of SIZE_PRESETS_MB) {
                expect(rotation_summary(mb, minutes)).not.toMatch(/null|NaN|undefined/);
            }
        }
    });
});

describe('rotation_warning', () => {
    /**
     * Closing a recording queues it for analysis, and these devices have one
     * core that is also keeping up with the radio. Rotating every few seconds
     * leaves the analyser permanently behind, which is a detector that has
     * stopped detecting.
     */
    it('warns about a time limit too short to analyse between', () => {
        expect(rotation_warning(null, 1)).toContain('little time to analyse');
        expect(rotation_warning(null, 4)).toBeTruthy();
    });

    it('warns about a size limit that would produce a flood of recordings', () => {
        expect(rotation_warning(1, null)).toContain('great many recordings');
    });

    it('stays quiet for every value the dropdown offers', () => {
        for (const minutes of TIME_PRESETS_MINUTES) {
            expect(rotation_warning(null, minutes)).toBeNull();
        }
        for (const mb of SIZE_PRESETS_MB) {
            expect(rotation_warning(mb, null)).toBeNull();
        }
    });

    it('stays quiet when rotation is off', () => {
        expect(rotation_warning(null, null)).toBeNull();
        expect(rotation_warning(0, 0)).toBeNull();
    });
});

describe('format_interval', () => {
    /**
     * "Every 1 hour" is what counting units gives you, and it is not what
     * anybody says. A quantity of one is left implied after "every".
     */
    it('leaves a quantity of one implied', () => {
        expect(format_interval(60)).toBe('hour');
        expect(format_interval(1440)).toBe('day');
        expect(format_interval(1)).toBe('minute');
    });

    it('keeps the count for everything else', () => {
        expect(format_interval(15)).toBe('15 minutes');
        expect(format_interval(120)).toBe('2 hours');
        expect(format_interval(720)).toBe('12 hours');
    });

    it('never says "1 hour" or "1 day" after every', () => {
        for (const minutes of TIME_PRESETS_MINUTES) {
            expect(`every ${format_interval(minutes)}`).not.toMatch(/every 1 /);
        }
    });
});
