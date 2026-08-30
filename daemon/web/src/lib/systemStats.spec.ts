import { describe, it, expect } from 'vitest';
import {
    load_per_core,
    load_state,
    format_uptime,
    hours_until_full,
    format_duration_hours,
    type HealthStats,
} from './systemStats';

const health = (load: number, cores = 1): HealthStats => ({
    uptime_secs: 0,
    load_avg: [load, load, load],
    cpu_count: cores,
});

describe('load', () => {
    /**
     * The core count is what makes the number mean anything. A load of 1.4
     * sounds mild and is saturation on the single core these devices have.
     */
    it('is judged against the number of cores', () => {
        expect(load_per_core(health(1.4, 1))).toBeCloseTo(1.4);
        expect(load_per_core(health(1.4, 4))).toBeCloseTo(0.35);
        expect(load_state(health(1.4, 1))).toBe('saturated');
        expect(load_state(health(1.4, 4))).toBe('idle');
    });

    it('names each band', () => {
        expect(load_state(health(0.2))).toBe('idle');
        expect(load_state(health(0.8))).toBe('busy');
        expect(load_state(health(1.5))).toBe('saturated');
        expect(load_state(health(3.0))).toBe('overloaded');
    });

    it('never divides by zero if a platform reports no cores', () => {
        expect(load_per_core({ ...health(2), cpu_count: 0 })).toBe(2);
    });
});

describe('format_uptime', () => {
    it('reads naturally at every scale', () => {
        expect(format_uptime(30)).toBe('30 seconds');
        expect(format_uptime(90)).toBe('1 minute');
        expect(format_uptime(3700)).toBe('1 hour 1 minute');
        expect(format_uptime(90000)).toBe('1 day 1 hour');
    });
});

describe('storage estimate', () => {
    it('estimates how long the free space will last', () => {
        // 100 KB/s with 3.6 GB free is ten hours.
        expect(hours_until_full(100_000, 3_600_000_000)).toBeCloseTo(10, 1);
    });

    /** A confident number from no evidence is worse than saying nothing. */
    it('declines to guess without a rate or a free space figure', () => {
        expect(hours_until_full(0, 1_000_000)).toBeNull();
        expect(hours_until_full(-5, 1_000_000)).toBeNull();
        expect(hours_until_full(100, undefined)).toBeNull();
    });

    it('describes the duration at a sensible scale', () => {
        expect(format_duration_hours(0.5)).toBe('30 minutes');
        expect(format_duration_hours(6)).toBe('6 hours');
        expect(format_duration_hours(100)).toBe('4 days');
    });
});
