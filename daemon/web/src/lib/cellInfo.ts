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
    /** RSRP the modem averaged over several samples. Steadier than the raw one. */
    avg_rsrp_dbm: number | null;
    /** RSRQ averaged the same way. Reported for neighbours only. */
    avg_rsrq_db: number | null;
}

export interface ServingCell {
    /** Level below which the modem starts hunting for a better cell. */
    search_threshold: number | null;
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
    /** Margin this neighbour has over the minimum level the network accepts. */
    s_rxlev: number | null;
}

export interface CellObservation {
    pci: number;
    earfcn: number;
    identity: CellIdentity | null;
    first_seen: string;
    last_seen: string;
    best_rsrp_dbm: number;
}

export interface EncryptionStatus {
    /** Cipher agreed for the radio link, as its 3GPP name. */
    rrc_cipher: string | null;
    /** Integrity algorithm for the radio link. */
    rrc_integrity: string | null;
    /** Cipher agreed with the core network. */
    nas_cipher: string | null;
    /** Integrity algorithm for core network signalling. */
    nas_integrity: string | null;
    last_seen: string;
}

/** Whether Rayhunter is understanding the traffic it sees. */
export interface DetectionHealth {
    messages_seen: number;
    messages_skipped: number;
    last_message: string | null;
}

/**
 * Identities this device has sent about itself.
 *
 * Absent unless switched on in the configuration. The web interface has no
 * authentication, so on a hotspot anything it serves is readable by anyone on
 * the WiFi, and an IMSI is the identifier an IMSI catcher exists to collect.
 */
export interface SubscriberIdentities {
    imsi?: string;
    imei?: string;
    imeisv?: string;
    tmsi?: string;
    imsi_sends: number;
    tmsi_changes: number;
}

/**
 * Whether the SIM appears to be working.
 *
 * A modem with no usable SIM still hears towers, because the broadcasts it
 * measures are transmitted to everyone. What it cannot do is hold a NAS
 * conversation with the network's core, so that is what separates a SIM the
 * network accepts from one it does not.
 */
export type SimVerdict = 'working' | 'registered' | 'not_registering' | 'searching';

export interface SimHealth {
    verdict: SimVerdict;
    /** A cellular interface carrying the default route, when there is one. */
    data_interface: string | null;
    nas_recent: boolean;
    last_nas_message: string | null;
    serving_cell: boolean;
    /** Minutes spent hearing towers without ever reaching the core. */
    silent_for_minutes: number | null;
}

export interface CellInfo {
    encryption?: EncryptionStatus;
    health: DetectionHealth;
    sim_health: SimHealth;
    serving: ServingCell | null;
    neighbors: NeighborCell[];
    history: CellObservation[];
    /** False before any measurement has arrived, which is normal when stopped. */
    has_data: boolean;
    /** Present only when the device is configured to disclose them. */
    identities?: SubscriberIdentities;
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

/**
 * How long ago a reading was taken, in words.
 *
 * Measurement reports only arrive while the modem is doing something, so a
 * reading can legitimately sit unchanged for a long time. Showing a bare
 * timestamp makes that look like a stalled page; showing its age makes it
 * clear the figure is real but old.
 */
export function reading_age(iso: string, now: number = Date.now()): string {
    const seconds = Math.max(0, Math.round((now - new Date(iso).getTime()) / 1000));
    if (seconds < 10) return 'just now';
    if (seconds < 60) return `${seconds} seconds ago`;
    const minutes = Math.round(seconds / 60);
    if (minutes < 60) return `${minutes} minute${minutes === 1 ? '' : 's'} ago`;
    const hours = Math.round(minutes / 60);
    return `${hours} hour${hours === 1 ? '' : 's'} ago`;
}

/** Readings older than this are called out as stale rather than current. */
export const STALE_AFTER_SECONDS = 120;

export function is_stale(iso: string, now: number = Date.now()): boolean {
    return (now - new Date(iso).getTime()) / 1000 > STALE_AFTER_SECONDS;
}

/** True when an algorithm name reports no protection at all. */
export function is_unprotected(algorithm: string | null | undefined): boolean {
    return algorithm !== null && algorithm !== undefined && algorithm.includes('none');
}

/** Kept for readability where the value is specifically a cipher. */
export const is_unencrypted = is_unprotected;

/**
 * Anything absent across both layers, named so a person can see what is
 * missing rather than having to compare four values themselves.
 */
export function missing_protections(status: EncryptionStatus | null | undefined): string[] {
    if (!status) return [];
    const missing: string[] = [];
    if (is_unprotected(status.rrc_cipher)) missing.push('radio link encryption');
    if (is_unprotected(status.rrc_integrity)) missing.push('radio link integrity');
    if (is_unprotected(status.nas_cipher)) missing.push('core network encryption');
    if (is_unprotected(status.nas_integrity)) missing.push('core network integrity');
    return missing;
}

/** Share of messages Rayhunter could not decode, 0 to 100. */
export function skipped_percent(health: DetectionHealth): number {
    if (!health.messages_seen) return 0;
    return (health.messages_skipped / health.messages_seen) * 100;
}

export type HealthVerdict = 'good' | 'degraded' | 'blind' | 'stalled' | 'starting';

/**
 * How much to trust a quiet screen.
 *
 * A detector understanding almost nothing looks identical to one seeing
 * nothing untoward, so the distinction has to be drawn explicitly. A stream
 * that has stopped arriving is worse still: everything else keeps looking
 * healthy while nothing is being examined at all.
 */
export function health_verdict(
    health: DetectionHealth,
    stalledAfterSeconds = 120,
    now: number = Date.now()
): HealthVerdict {
    if (!health.messages_seen) return 'starting';
    if (health.last_message) {
        const age = (now - new Date(health.last_message).getTime()) / 1000;
        if (age > stalledAfterSeconds) return 'stalled';
    }
    const missed = skipped_percent(health);
    if (missed > 50) return 'blind';
    if (missed > 10) return 'degraded';
    return 'good';
}

/**
 * The SIM verdict in words, for people who did not come here to learn what NAS
 * is.
 *
 * `tone` is 'bad' only for the case that actually needs acting on. A SIM that
 * registers and is given no data is fine for Rayhunter's purposes, and saying
 * otherwise would send people hunting for a fault that is not there.
 */
export function sim_health_summary(health: SimHealth | undefined): {
    label: string;
    detail: string;
    tone: 'good' | 'bad' | 'unknown';
} {
    switch (health?.verdict) {
        case 'working':
            return {
                label: 'SIM is working',
                detail: health.data_interface
                    ? `Registered with the network and carrying data on ${health.data_interface}.`
                    : 'Registered with the network and carrying data.',
                tone: 'good',
            };
        case 'registered':
            return {
                label: 'SIM is working',
                detail: health.nas_recent
                    ? 'The network is talking to this SIM. There is no data connection, which is normal for a SIM bought only for this, and does not affect what Rayhunter can see.'
                    : 'The network accepted this SIM earlier. It has been quiet since, which is what an idle device looks like.',
                tone: 'good',
            };
        case 'not_registering': {
            const minutes = health.silent_for_minutes;
            const howLong = minutes && minutes > 0 ? ` for ${minutes} minutes` : '';
            return {
                label: 'SIM may not be working',
                detail: `Towers are being heard${howLong}, but this SIM has never been accepted by the network. A modem picks up towers with no SIM at all, so this is what a dead, unactivated or badly seated SIM looks like. Check the SIM is seated, and that it is active.`,
                tone: 'bad',
            };
        }
        default:
            return {
                label: 'Checking the SIM',
                detail: 'Nothing heard from the network yet. This is normal for the first minute or so, and while recording is stopped.',
                tone: 'unknown',
            };
    }
}

/**
 * Moments where the tracking area changed between consecutive observations.
 *
 * Crossing a tracking area boundary makes a phone announce itself to the
 * network, which is one of the few moments surveillance equipment can rely on
 * hearing from a device that would otherwise stay quiet.
 */
export function tracking_area_changes(
    history: CellObservation[]
): { at: string; from: number | null; to: number | null }[] {
    // History arrives newest first; walk it oldest first so changes read forwards.
    const ordered = [...history].sort(
        (a, b) => new Date(a.first_seen).getTime() - new Date(b.first_seen).getTime()
    );
    const changes: { at: string; from: number | null; to: number | null }[] = [];
    let previous: number | null | undefined;
    for (const entry of ordered) {
        const tac = entry.identity?.tac ?? null;
        if (tac === null) continue;
        if (previous !== undefined && previous !== tac) {
            changes.push({ at: entry.first_seen, from: previous, to: tac });
        }
        previous = tac;
    }
    return changes;
}
