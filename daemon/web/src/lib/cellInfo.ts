/**
 * What the modem currently sees on the air.
 *
 * Mirrors the CellInfo types in daemon/src/cell_info.rs. Only updates while a
 * recording is running, because it comes from the modem diagnostic stream.
 */

export interface CellIdentity {
    /** Mobile Country Code as digits, e.g. "311" for the United States. */
    mcc: string | null;
    /**
     * Mobile Network Code as digits. Kept as digits because an MNC may be two
     * or three long and the length is part of the identity, so "30" and "030"
     * are different networks.
     */
    mnc: string | null;
    /** The 28 bit cell identity from the tower's broadcast. */
    cell_id: number | null;
    /** Tracking area code, the grouping the network uses to page your phone. */
    tac: number | null;
}

export interface SignalMeasurements {
    rsrp_dbm: number;
    rsrq_db: number;
    rssi_dbm: number;
}

export interface ServingCell {
    pci: number;
    earfcn: number;
    band: number | null;
    signal: SignalMeasurements;
    identity: CellIdentity | null;
    /** Raw timing advance from the last random access, if one has happened. */
    timing_advance: number | null;
    last_seen: string;
}

/**
 * A neighbouring cell. Deliberately has no identity: neighbours are only ever
 * reported by physical cell identity, which is reused across a network and does
 * not name a tower.
 */
export interface NeighborCell {
    pci: number;
    earfcn: number;
    signal: SignalMeasurements;
}

export interface CellObservation {
    pci: number;
    earfcn: number;
    identity: CellIdentity | null;
    first_seen: string;
    last_seen: string;
    best_rsrp_dbm: number;
}

export interface CellInfo {
    serving: ServingCell | null;
    neighbors: NeighborCell[];
    history: CellObservation[];
    /** False before any measurement has arrived, which is normal when stopped. */
    has_data: boolean;
}

export async function get_cell_info(): Promise<CellInfo> {
    const response = await fetch('/api/cell-info');
    if (!response.ok) throw new Error(await response.text());
    return response.json();
}

/** How strong a signal is, in words. Thresholds are the usual LTE rules of thumb. */
export type SignalQuality = 'excellent' | 'good' | 'fair' | 'poor';

export function rsrp_quality(rsrp_dbm: number): SignalQuality {
    if (rsrp_dbm >= -80) return 'excellent';
    if (rsrp_dbm >= -90) return 'good';
    if (rsrp_dbm >= -100) return 'fair';
    return 'poor';
}

/**
 * The operator name for well known network codes, so the numbers mean something
 * without a lookup. Returns null rather than guessing for anything unlisted.
 */
export function operator_name(mcc: string | null, mnc: string | null): string | null {
    if (mcc === null || mnc === null) return null;
    const known: Record<string, string> = {
        '310-260': 'T-Mobile US',
        '310-410': 'AT&T',
        '311-480': 'Verizon',
        '310-120': 'Sprint',
        '312-530': 'Sprint',
        '310-030': 'AT&T',
        '310-150': 'AT&T',
        '310-170': 'AT&T',
        '310-280': 'AT&T',
        '311-660': 'T-Mobile US',
        '310-200': 'T-Mobile US',
        '310-210': 'T-Mobile US',
        '310-220': 'T-Mobile US',
        '310-230': 'T-Mobile US',
        '310-240': 'T-Mobile US',
        '310-250': 'T-Mobile US',
        '310-270': 'T-Mobile US',
        '310-310': 'T-Mobile US',
        '310-490': 'T-Mobile US',
        '310-590': 'T-Mobile US',
        '310-640': 'T-Mobile US',
        '310-800': 'T-Mobile US',
        '302-220': 'Telus (CA)',
        '302-610': 'Bell (CA)',
        '302-720': 'Rogers (CA)',
        '234-10': 'O2 (UK)',
        '234-15': 'Vodafone (UK)',
        '234-20': '3 (UK)',
        '234-30': 'EE (UK)',
    };
    return known[`${mcc}-${mnc}`] ?? null;
}

/** The network identity as it is normally written down, e.g. "311-480". */
export function format_plmn(mcc: string | null, mnc: string | null): string | null {
    if (mcc === null || mnc === null) return null;
    return `${mcc}-${mnc}`;
}

/**
 * The eNodeB (base station) and sector within it.
 *
 * An LTE cell identity packs both: the top 20 bits identify the base station
 * and the bottom 8 identify which sector of it you are on. Splitting them is
 * what lets you tell "moved to another face of the same tower" apart from
 * "moved to a different tower".
 */
export function split_cell_id(cell_id: number): { enodeb_id: number; sector_id: number } {
    return { enodeb_id: Math.floor(cell_id / 256), sector_id: cell_id % 256 };
}
