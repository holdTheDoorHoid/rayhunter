import { describe, it, expect } from 'vitest';
import {
    LTE_BANDS,
    band_for_earfcn,
    downlink_mhz,
    uplink_earfcn,
    uplink_mhz,
    signal_bars,
    asu_from_rsrp,
    timing_advance_metres,
} from './lteBands';

describe('band lookup', () => {
    it('identifies the band the test device is actually on', () => {
        // Read off an Orbic RC400L attached to a live network.
        const b = band_for_earfcn(67086);
        expect(b?.band).toBe(66);
        expect(b?.name).toBe('AWS-3');
    });

    it('returns nothing for a channel outside every known band', () => {
        expect(band_for_earfcn(999999)).toBeNull();
        // A gap between defined ranges must not be attributed to a neighbour.
        expect(band_for_earfcn(2700)).toBeNull();
    });

    it('has no overlapping channel ranges', () => {
        const sorted = [...LTE_BANDS].sort((a, b) => a.dlOffset - b.dlOffset);
        for (let i = 1; i < sorted.length; i++) {
            expect(
                sorted[i].dlOffset,
                `band ${sorted[i].band} overlaps ${sorted[i - 1].band}`
            ).toBeGreaterThan(sorted[i - 1].dlHigh);
        }
    });

    it('gives every band a low frequency below its high channel frequency', () => {
        for (const b of LTE_BANDS) {
            expect(b.dlHigh, `band ${b.band}`).toBeGreaterThanOrEqual(b.dlOffset);
        }
    });
});

describe('frequency derivation', () => {
    it('computes the downlink frequency for the test device', () => {
        // 2110 + 0.1 * (67086 - 66436) = 2175 MHz
        expect(downlink_mhz(67086)).toBe(2175);
    });

    it('matches known band edges', () => {
        expect(downlink_mhz(0)).toBe(2110); // band 1 lowest
        expect(downlink_mhz(600)).toBe(1930); // band 2 lowest
        expect(downlink_mhz(5180)).toBe(746); // band 13 lowest
    });

    it('pairs the uplink channel with the downlink one', () => {
        // Band 66: uplink offset 131972 against downlink offset 66436.
        expect(uplink_earfcn(67086)).toBe(132622);
        expect(uplink_mhz(67086)).toBe(1775);
    });

    it('uses one frequency for both directions on time division bands', () => {
        const tdd = 39700; // band 41
        expect(uplink_earfcn(tdd)).toBe(tdd);
        expect(uplink_mhz(tdd)).toBe(downlink_mhz(tdd));
    });

    it('returns nothing rather than guessing outside known bands', () => {
        expect(downlink_mhz(999999)).toBeNull();
        expect(uplink_mhz(999999)).toBeNull();
    });
});

describe('signal readings', () => {
    it('maps strength to bars the way phones do', () => {
        expect(signal_bars(-70)).toBe(4);
        expect(signal_bars(-90)).toBe(3);
        expect(signal_bars(-100)).toBe(2);
        expect(signal_bars(-110)).toBe(1);
        expect(signal_bars(-125)).toBe(0);
    });

    it('computes ASU as Android does, and clamps it', () => {
        expect(asu_from_rsrp(-101)).toBe(39);
        expect(asu_from_rsrp(-140)).toBe(0);
        expect(asu_from_rsrp(-200)).toBe(0);
        expect(asu_from_rsrp(0)).toBe(97);
    });

    it('turns timing advance into an approximate distance', () => {
        expect(timing_advance_metres(0)).toBe(0);
        expect(timing_advance_metres(1)).toBe(78);
        expect(timing_advance_metres(32)).toBe(2498);
    });
});
