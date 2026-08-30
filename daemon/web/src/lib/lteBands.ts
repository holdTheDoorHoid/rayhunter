/**
 * LTE band data, for turning a channel number into something meaningful.
 *
 * A raw EARFCN says nothing to most people and little even to those who know
 * the term. The band, its common name, and the actual frequency in MHz are what
 * make it possible to say "this tower is on the band my operator uses here" or
 * to notice one that is not.
 *
 * Frequencies follow 3GPP TS 36.101:
 *   F_DL = F_DL_low + 0.1 * (N_DL - N_Offs-DL)
 *   F_UL = F_UL_low + 0.1 * (N_UL - N_Offs-UL)
 */

export interface LteBand {
    band: number;
    /** The name people actually use for this band. */
    name: string;
    /** Lowest downlink frequency of the band, MHz. */
    dlLowMhz: number;
    /** First downlink channel number in the band. */
    dlOffset: number;
    /** Last downlink channel number in the band. */
    dlHigh: number;
    /** Lowest uplink frequency, MHz. Absent for downlink only bands. */
    ulLowMhz?: number;
    /** First uplink channel number. Absent for downlink only bands. */
    ulOffset?: number;
    /** Time division bands transmit and receive on one frequency. */
    tdd?: boolean;
}

export const LTE_BANDS: LteBand[] = [
    {
        band: 1,
        name: '2100 IMT',
        dlLowMhz: 2110,
        dlOffset: 0,
        dlHigh: 599,
        ulLowMhz: 1920,
        ulOffset: 18000,
    },
    {
        band: 2,
        name: '1900 PCS',
        dlLowMhz: 1930,
        dlOffset: 600,
        dlHigh: 1199,
        ulLowMhz: 1850,
        ulOffset: 18600,
    },
    {
        band: 3,
        name: '1800 DCS',
        dlLowMhz: 1805,
        dlOffset: 1200,
        dlHigh: 1949,
        ulLowMhz: 1710,
        ulOffset: 19200,
    },
    {
        band: 4,
        name: 'AWS-1',
        dlLowMhz: 2110,
        dlOffset: 1950,
        dlHigh: 2399,
        ulLowMhz: 1710,
        ulOffset: 19950,
    },
    {
        band: 5,
        name: '850 CLR',
        dlLowMhz: 869,
        dlOffset: 2400,
        dlHigh: 2649,
        ulLowMhz: 824,
        ulOffset: 20400,
    },
    {
        band: 7,
        name: '2600 IMT-E',
        dlLowMhz: 2620,
        dlOffset: 2750,
        dlHigh: 3449,
        ulLowMhz: 2500,
        ulOffset: 20750,
    },
    {
        band: 8,
        name: '900 GSM',
        dlLowMhz: 925,
        dlOffset: 3450,
        dlHigh: 3799,
        ulLowMhz: 880,
        ulOffset: 21450,
    },
    {
        band: 12,
        name: '700 a/b/c',
        dlLowMhz: 729,
        dlOffset: 5010,
        dlHigh: 5179,
        ulLowMhz: 699,
        ulOffset: 23010,
    },
    {
        band: 13,
        name: '700 c',
        dlLowMhz: 746,
        dlOffset: 5180,
        dlHigh: 5279,
        ulLowMhz: 777,
        ulOffset: 23180,
    },
    {
        band: 14,
        name: '700 PS',
        dlLowMhz: 758,
        dlOffset: 5280,
        dlHigh: 5379,
        ulLowMhz: 788,
        ulOffset: 23280,
    },
    {
        band: 17,
        name: '700 b/c',
        dlLowMhz: 734,
        dlOffset: 5730,
        dlHigh: 5849,
        ulLowMhz: 704,
        ulOffset: 23730,
    },
    {
        band: 18,
        name: '800 Lower',
        dlLowMhz: 860,
        dlOffset: 5850,
        dlHigh: 5999,
        ulLowMhz: 815,
        ulOffset: 23850,
    },
    {
        band: 19,
        name: '800 Upper',
        dlLowMhz: 875,
        dlOffset: 6000,
        dlHigh: 6149,
        ulLowMhz: 830,
        ulOffset: 24000,
    },
    {
        band: 20,
        name: '800 DD',
        dlLowMhz: 791,
        dlOffset: 6150,
        dlHigh: 6449,
        ulLowMhz: 832,
        ulOffset: 24150,
    },
    {
        band: 21,
        name: '1500 Upper',
        dlLowMhz: 1495.9,
        dlOffset: 6450,
        dlHigh: 6599,
        ulLowMhz: 1447.9,
        ulOffset: 24450,
    },
    {
        band: 25,
        name: '1900 PCS-G',
        dlLowMhz: 1930,
        dlOffset: 8040,
        dlHigh: 8689,
        ulLowMhz: 1850,
        ulOffset: 26040,
    },
    {
        band: 26,
        name: '850+',
        dlLowMhz: 859,
        dlOffset: 8690,
        dlHigh: 9039,
        ulLowMhz: 814,
        ulOffset: 26690,
    },
    {
        band: 28,
        name: '700 APT',
        dlLowMhz: 758,
        dlOffset: 9210,
        dlHigh: 9659,
        ulLowMhz: 703,
        ulOffset: 27210,
    },
    {
        band: 30,
        name: '2300 WCS',
        dlLowMhz: 2350,
        dlOffset: 9770,
        dlHigh: 9869,
        ulLowMhz: 2305,
        ulOffset: 27660,
    },
    { band: 38, name: 'TD 2600', dlLowMhz: 2570, dlOffset: 37750, dlHigh: 38249, tdd: true },
    { band: 39, name: 'TD 1900', dlLowMhz: 1880, dlOffset: 38250, dlHigh: 38649, tdd: true },
    { band: 40, name: 'TD 2300', dlLowMhz: 2300, dlOffset: 38650, dlHigh: 39649, tdd: true },
    { band: 41, name: 'TD 2500', dlLowMhz: 2496, dlOffset: 39650, dlHigh: 41589, tdd: true },
    { band: 42, name: 'TD 3500', dlLowMhz: 3400, dlOffset: 41590, dlHigh: 43589, tdd: true },
    { band: 43, name: 'TD 3700', dlLowMhz: 3600, dlOffset: 43590, dlHigh: 45589, tdd: true },
    { band: 48, name: 'CBRS 3500', dlLowMhz: 3550, dlOffset: 55240, dlHigh: 56739, tdd: true },
    {
        band: 66,
        name: 'AWS-3',
        dlLowMhz: 2110,
        dlOffset: 66436,
        dlHigh: 67335,
        ulLowMhz: 1710,
        ulOffset: 131972,
    },
    {
        band: 71,
        name: '600',
        dlLowMhz: 617,
        dlOffset: 68586,
        dlHigh: 68935,
        ulLowMhz: 663,
        ulOffset: 132672,
    },
];

export function band_for_earfcn(earfcn: number): LteBand | null {
    return LTE_BANDS.find((b) => earfcn >= b.dlOffset && earfcn <= b.dlHigh) ?? null;
}

/** Downlink frequency in MHz, or null when the channel is outside known bands. */
export function downlink_mhz(earfcn: number): number | null {
    const b = band_for_earfcn(earfcn);
    if (!b) return null;
    return Math.round((b.dlLowMhz + 0.1 * (earfcn - b.dlOffset)) * 10) / 10;
}

/**
 * The uplink channel number paired with a downlink one.
 *
 * Time division bands use a single frequency for both directions, so the
 * channel number is unchanged there.
 */
export function uplink_earfcn(earfcn: number): number | null {
    const b = band_for_earfcn(earfcn);
    if (!b) return null;
    if (b.tdd) return earfcn;
    if (b.ulOffset === undefined) return null;
    return earfcn - b.dlOffset + b.ulOffset;
}

/** Uplink frequency in MHz. */
export function uplink_mhz(earfcn: number): number | null {
    const b = band_for_earfcn(earfcn);
    if (!b) return null;
    if (b.tdd) return downlink_mhz(earfcn);
    if (b.ulLowMhz === undefined || b.ulOffset === undefined) return null;
    const ul = uplink_earfcn(earfcn);
    if (ul === null) return null;
    return Math.round((b.ulLowMhz + 0.1 * (ul - b.ulOffset)) * 10) / 10;
}

/**
 * Signal strength as bars, 0 to 4, matching the thresholds phones use.
 * Purely a reading aid; the dBm figure is the real measurement.
 */
export function signal_bars(rsrp_dbm: number): number {
    if (rsrp_dbm >= -85) return 4;
    if (rsrp_dbm >= -95) return 3;
    if (rsrp_dbm >= -105) return 2;
    if (rsrp_dbm >= -115) return 1;
    return 0;
}

/**
 * Arbitrary Strength Unit, as Android reports it for LTE: RSRP + 140,
 * clamped to the 0 to 97 range the API defines.
 */
export function asu_from_rsrp(rsrp_dbm: number): number {
    return Math.max(0, Math.min(97, Math.round(rsrp_dbm) + 140));
}

/**
 * Rough distance to the tower implied by a timing advance value, in metres.
 *
 * Each LTE timing advance step is 16 * Ts, about 0.52 microseconds of round
 * trip, so roughly 78 metres of separation. This is a line of sight estimate
 * and reflects signal path rather than map distance, so it reads high wherever
 * the signal bounces.
 */
export function timing_advance_metres(ta: number): number {
    return Math.round(ta * 78.07);
}
