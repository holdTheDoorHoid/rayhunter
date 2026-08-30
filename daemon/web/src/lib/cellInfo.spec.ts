import { describe, it, expect } from 'vitest';
import {
    rsrp_quality,
    operator_name,
    format_plmn,
    split_cell_id,
    reading_age,
    is_stale,
    is_unencrypted,
    skipped_percent,
    health_verdict,
    tracking_area_changes,
} from './cellInfo';

describe('rsrp_quality', () => {
    it('describes signal strength in the usual LTE bands', () => {
        expect(rsrp_quality(-70)).toBe('excellent');
        expect(rsrp_quality(-80)).toBe('excellent');
        expect(rsrp_quality(-85)).toBe('good');
        expect(rsrp_quality(-95)).toBe('fair');
        expect(rsrp_quality(-110)).toBe('poor');
    });
});

describe('operator_name', () => {
    it('names operators from their network codes', () => {
        // The values read off the test device on a live network.
        expect(operator_name('311', '480')).toBe('Verizon');
        expect(operator_name('310', '260')).toBe('T-Mobile US');
        expect(operator_name('310', '410')).toBe('AT&T');
    });

    it('returns null rather than guessing at an unknown network', () => {
        expect(operator_name('999', '999')).toBeNull();
        expect(operator_name(null, '480')).toBeNull();
        expect(operator_name('311', null)).toBeNull();
    });
});

describe('format_plmn', () => {
    it('writes the network identity the usual way', () => {
        expect(format_plmn('311', '480')).toBe('311-480');
    });

    /**
     * A two digit and a three digit MNC are different networks, so the digits
     * are carried through exactly rather than being turned into a number.
     */
    it('preserves a leading zero rather than normalising it away', () => {
        expect(format_plmn('310', '030')).toBe('310-030');
        expect(format_plmn('310', '30')).toBe('310-30');
    });

    it('is empty when either half is unknown', () => {
        expect(format_plmn(null, '480')).toBeNull();
    });
});

describe('split_cell_id', () => {
    /**
     * Splitting matters: a change of sector means you moved around one tower,
     * a change of base station means you moved to a different one.
     */
    it('separates the base station from the sector', () => {
        // The identity read off the test device.
        expect(split_cell_id(25316666)).toEqual({ enodeb_id: 98893, sector_id: 58 });
    });

    it('keeps sectors of one base station together', () => {
        const a = split_cell_id(25316666);
        const b = split_cell_id(25316667);
        expect(a.enodeb_id).toBe(b.enodeb_id);
        expect(a.sector_id).not.toBe(b.sector_id);
    });
});

describe('reading age', () => {
    const base = new Date('2026-08-30T12:00:00Z').getTime();
    const at = (secondsAgo: number) => new Date(base - secondsAgo * 1000).toISOString();

    it('describes how old a reading is in words', () => {
        expect(reading_age(at(0), base)).toBe('just now');
        expect(reading_age(at(30), base)).toBe('30 seconds ago');
        expect(reading_age(at(60), base)).toBe('1 minute ago');
        expect(reading_age(at(600), base)).toBe('10 minutes ago');
        expect(reading_age(at(7200), base)).toBe('2 hours ago');
    });

    /**
     * Measurement reports stop while a modem sits idle, so a reading can be
     * genuinely old. Saying so is what stops the panel looking stalled.
     */
    it('calls a reading stale only once it really is', () => {
        expect(is_stale(at(10), base)).toBe(false);
        expect(is_stale(at(119), base)).toBe(false);
        expect(is_stale(at(300), base)).toBe(true);
    });

    it('never reports a negative age from clock skew', () => {
        expect(reading_age(at(-30), base)).toBe('just now');
    });
});

describe('encryption', () => {
    it('recognises the absence of encryption from the cipher name', () => {
        expect(is_unencrypted('EEA0 (none)')).toBe(true);
        expect(is_unencrypted('EEA2 (AES)')).toBe(false);
        expect(is_unencrypted(null)).toBe(false);
        expect(is_unencrypted(undefined)).toBe(false);
    });
});

describe('detection health', () => {
    const health = (seen: number, skipped: number, last: string | null = null) => ({
        messages_seen: seen,
        messages_skipped: skipped,
        last_message: last,
    });
    const now = new Date('2026-08-30T12:00:00Z').getTime();
    const ago = (s: number) => new Date(now - s * 1000).toISOString();

    it('computes the share missed without dividing by zero', () => {
        expect(skipped_percent(health(0, 0))).toBe(0);
        expect(skipped_percent(health(100, 5))).toBe(5);
    });

    /**
     * The distinction the whole feature exists for: understanding nothing must
     * not look like seeing nothing untoward.
     */
    it('separates a healthy quiet night from a blind detector', () => {
        expect(health_verdict(health(1000, 5, ago(5)), 120, now)).toBe('good');
        expect(health_verdict(health(1000, 200, ago(5)), 120, now)).toBe('degraded');
        expect(health_verdict(health(1000, 800, ago(5)), 120, now)).toBe('blind');
    });

    it('calls out a stream that has stopped arriving', () => {
        expect(health_verdict(health(1000, 0, ago(600)), 120, now)).toBe('stalled');
    });

    it('says it is starting rather than healthy before anything arrives', () => {
        expect(health_verdict(health(0, 0), 120, now)).toBe('starting');
    });
});

describe('tracking_area_changes', () => {
    const obs = (tac: number | null, first: string) => ({
        pci: 1,
        earfcn: 100,
        identity: tac === null ? null : { mcc: '001', mnc: '01', cell_id: 1, tac },
        first_seen: first,
        last_seen: first,
        best_rsrp_dbm: -90,
    });

    it('reports each crossing, oldest first', () => {
        const changes = tracking_area_changes([
            obs(200, '2026-08-30T12:02:00Z'),
            obs(100, '2026-08-30T12:00:00Z'),
            obs(300, '2026-08-30T12:04:00Z'),
        ]);
        expect(changes.map((c) => [c.from, c.to])).toEqual([
            [100, 200],
            [200, 300],
        ]);
    });

    it('reports nothing when the area never changed', () => {
        expect(
            tracking_area_changes([
                obs(100, '2026-08-30T12:00:00Z'),
                obs(100, '2026-08-30T12:02:00Z'),
            ])
        ).toEqual([]);
    });

    it('ignores cells whose identity was never captured', () => {
        expect(
            tracking_area_changes([
                obs(null, '2026-08-30T12:00:00Z'),
                obs(null, '2026-08-30T12:02:00Z'),
            ])
        ).toEqual([]);
    });
});
