import { describe, it, expect } from 'vitest';
import {
    load_per_core,
    cpu_state,
    load_state_label,
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

describe('load average', () => {
    it('is still reported per core, since raw load means little alone', () => {
        expect(load_per_core(health(1.4, 1))).toBeCloseTo(1.4);
        expect(load_per_core(health(1.4, 4))).toBeCloseTo(0.35);
    });

    it('never divides by zero if a platform reports no cores', () => {
        expect(load_per_core({ ...health(2), cpu_count: 0 })).toBe(2);
    });
});

describe('cpu_state', () => {
    /**
     * The distinction that matters. Measured on the test device: load average
     * 1.59 on a single core, which reads as saturated, while the processor was
     * 81% idle. Judging on load would report a problem that is not there.
     */
    it('judges on measured usage, not on load average', () => {
        const misleading = { ...health(1.59, 1), cpu_busy_percent: 19 };
        expect(load_per_core(misleading)).toBeCloseTo(1.59);
        expect(cpu_state(misleading)).toBe('comfortable');
    });

    it('names each band', () => {
        expect(cpu_state({ ...health(0), cpu_busy_percent: 10 })).toBe('comfortable');
        expect(cpu_state({ ...health(0), cpu_busy_percent: 60 })).toBe('busy');
        expect(cpu_state({ ...health(0), cpu_busy_percent: 80 })).toBe('stretched');
        expect(cpu_state({ ...health(0), cpu_busy_percent: 95 })).toBe('overloaded');
    });

    it('says nothing when usage has not been measured yet', () => {
        expect(cpu_state(health(1.5))).toBeNull();
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

describe('load_state_label', () => {
    it('starts the state with a capital, since it begins a line on screen', () => {
        expect(load_state_label('comfortable')).toBe('Comfortable');
        expect(load_state_label('busy')).toBe('Busy');
        expect(load_state_label('stretched')).toBe('Stretched');
        expect(load_state_label('overloaded')).toBe('Overloaded');
    });
});
