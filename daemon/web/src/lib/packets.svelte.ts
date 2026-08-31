/**
 * Reading the messages inside a stored recording.
 *
 * Mirrors daemon/src/packet_explorer.rs. Nothing is decoded here: the device
 * runs the recording back through the same path the heuristics used, so what
 * appears on screen is the interpretation that produced the warning rather
 * than a second opinion from a different decoder.
 */

export interface PacketSummary {
    packet_num: number;
    timestamp: string | null;
    /** "LTE RRC", "LTE NAS", or the radio technology when it is not LTE. */
    protocol: string;
    /** Logical channel for RRC, absent for NAS. */
    channel: string | null;
    direction: string | null;
    message_type: string | null;
    payload_len: number;
    parse_status: 'decoded' | 'undecodable' | 'not a signalling message';
}

export interface PacketDetail extends PacketSummary {
    decoded: string | null;
    decode_error: string | null;
    raw_hex: string | null;
    raw_truncated: boolean;
}

export interface PacketList {
    packets: PacketSummary[];
    first_packet_num: number;
    reached_end: boolean;
}

export async function list_packets(
    recording: string,
    params: { offset?: number; limit?: number; around?: number; context?: number } = {}
): Promise<PacketList> {
    const query = Object.entries(params)
        .filter(([, v]) => v !== undefined)
        .map(([k, v]) => `${k}=${v}`)
        .join('&');
    const response = await fetch(`/api/packets/${recording}${query ? `?${query}` : ''}`);
    if (!response.ok) throw new Error(await response.text());
    return response.json();
}

export async function get_packet(recording: string, packetNum: number): Promise<PacketDetail> {
    const response = await fetch(`/api/packets/${recording}/${packetNum}`);
    if (!response.ok) throw new Error(await response.text());
    return response.json();
}

export type ProtocolFilter = 'all' | 'rrc' | 'nas';
export type DirectionFilter = 'all' | 'uplink' | 'downlink';

export interface PacketFilters {
    protocol: ProtocolFilter;
    direction: DirectionFilter;
    /** Hide messages that carried no signalling, which are the bulk of a capture. */
    decodedOnly: boolean;
    /** Free text against message name, channel and protocol. */
    search: string;
}

export const DEFAULT_FILTERS: PacketFilters = {
    protocol: 'all',
    direction: 'all',
    decodedOnly: true,
    search: '',
};

/**
 * Filtering happens in the browser over the window already fetched.
 *
 * Filtering on the device would mean decoding the whole recording per request
 * on hardware with one slow core, so the window is fetched and narrowed here.
 * The consequence, which the UI states rather than hides, is that a filter
 * applies to the packets on screen and not to the entire recording.
 */
export function apply_filters(packets: PacketSummary[], filters: PacketFilters): PacketSummary[] {
    const needle = filters.search.trim().toLowerCase();
    return packets.filter((p) => {
        if (filters.decodedOnly && p.parse_status !== 'decoded') return false;
        if (filters.protocol === 'rrc' && !p.protocol.includes('RRC')) return false;
        if (filters.protocol === 'nas' && !p.protocol.includes('NAS')) return false;
        if (filters.direction !== 'all' && p.direction !== filters.direction) return false;
        if (needle) {
            const haystack = [p.protocol, p.channel, p.message_type, String(p.packet_num)]
                .filter(Boolean)
                .join(' ')
                .toLowerCase();
            if (!haystack.includes(needle)) return false;
        }
        return true;
    });
}

/** An arrow for the direction column, since the words are long and repetitive. */
export function direction_arrow(direction: string | null): string {
    if (direction === 'uplink') return '↑';
    if (direction === 'downlink') return '↓';
    return '';
}
