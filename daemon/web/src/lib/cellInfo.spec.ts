import { describe, it, expect } from 'vitest';
import {
    rsrp_quality,
    operator_name,
    format_plmn,
    split_cell_id,
    reading_age,
    is_stale,
    is_unprotected,
    missing_protections,
    skipped_percent,
    health_verdict,
    tracking_area_changes,
    sim_health_summary,
    type SimHealth,
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

describe('protection', () => {
    const status = (
        rrc_cipher: string | null,
        rrc_integrity: string | null,
        nas_cipher: string | null,
        nas_integrity: string | null
    ) => ({
        rrc_cipher,
        rrc_integrity,
        nas_cipher,
        nas_integrity,
        last_seen: '2026-08-30T12:00:00Z',
    });

    it('recognises the absence of a protection from its name', () => {
        expect(is_unprotected('EEA0 (none)')).toBe(true);
        expect(is_unprotected('EIA0 (none)')).toBe(true);
        expect(is_unprotected('EEA2 (AES)')).toBe(false);
        expect(is_unprotected(null)).toBe(false);
        expect(is_unprotected(undefined)).toBe(false);
    });

    it('says nothing is missing when all four are real', () => {
        expect(
            missing_protections(status('EEA2 (AES)', 'EIA2 (AES)', 'EEA2 (AES)', 'EIA2 (AES)'))
        ).toEqual([]);
    });

    /**
     * The two layers are separate agreements, and integrity is a different
     * guarantee from encryption, so each has to be reported on its own rather
     * than collapsed into one verdict.
     */
    it('names exactly which protections are absent', () => {
        expect(
            missing_protections(status('EEA2 (AES)', 'EIA0 (none)', 'EEA0 (none)', 'EIA2 (AES)'))
        ).toEqual(['radio link integrity', 'core network encryption']);
    });

    it('treats a value not yet seen as unknown rather than missing', () => {
        expect(missing_protections(status(null, null, null, null))).toEqual([]);
        expect(missing_protections(null)).toEqual([]);
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

describe('sim_health_summary', () => {
    const base: SimHealth = {
        verdict: 'searching',
        data_interface: null,
        nas_recent: false,
        last_nas_message: null,
        serving_cell: false,
        silent_for_minutes: null,
    };

    it('calls a SIM carrying data good, and names the interface', () => {
        const s = sim_health_summary({
            ...base,
            verdict: 'working',
            data_interface: 'rmnet_data0',
        });
        expect(s.tone).toBe('good');
        expect(s.detail).toContain('rmnet_data0');
    });

    /* Registering without a data bearer is normal for a SIM bought only for
       this, and must not be reported as a fault. */
    it('does not call a registered SIM without data a problem', () => {
        const s = sim_health_summary({ ...base, verdict: 'registered', nas_recent: true });
        expect(s.tone).toBe('good');
        expect(s.label).toBe('SIM is working');
    });

    it('warns when towers are heard but the SIM never registers', () => {
        const s = sim_health_summary({
            ...base,
            verdict: 'not_registering',
            serving_cell: true,
            silent_for_minutes: 42,
        });
        expect(s.tone).toBe('bad');
        expect(s.detail).toContain('42 minutes');
    });

    /* An absent field must not render as a confident verdict. */
    it('is unknown when there is nothing to go on', () => {
        expect(sim_health_summary(undefined).tone).toBe('unknown');
        expect(sim_health_summary({ ...base }).tone).toBe('unknown');
    });
});
