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
    /** Physical cell identity of the tower involved. */
    pci: number | null;
    earfcn: number | null;
    /** System frame number, the radio's own 10ms clock. */
    sfn: number | null;
    subfn: number | null;
}

export interface PacketDetail extends PacketSummary {
    decoded: string | null;
    decode_error: string | null;
    raw_hex: string | null;
    raw_truncated: boolean;
    /** Bytes of modem framing wrapped around the protocol data unit. */
    framing_len: number | null;
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
            const haystack = [
                p.protocol,
                p.channel,
                p.message_type,
                String(p.packet_num),
                p.pci !== null ? `pci${p.pci}` : null,
            ]
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

/**
 * Cells appearing in a set of packets, strongest first by how often they occur.
 *
 * A capture mixes messages from every cell in range. Knowing which cells are
 * present, and how much traffic came from each, is often the fastest way to
 * notice one that should not be there.
 */
export function cells_present(packets: PacketSummary[]): { pci: number; count: number }[] {
    const counts = new Map<number, number>();
    for (const p of packets) {
        if (p.pci === null) continue;
        counts.set(p.pci, (counts.get(p.pci) ?? 0) + 1);
    }
    return [...counts.entries()]
        .map(([pci, count]) => ({ pci, count }))
        .sort((a, b) => b.count - a.count);
}

/** What opening the explorer should do, if anything. */
export type OpenAction = { kind: 'nothing' } | { kind: 'focus'; packet: number } | { kind: 'page' };

/**
 * Decide how the explorer should respond to being opened.
 *
 * This is a pure function on purpose. The caller is a Svelte effect, and
 * effects re-run whenever anything they touch is invalidated, which is not
 * the same as the person having asked for something. An earlier version acted
 * every time it ran and so refetched the same window forever, which pins a
 * device that has one core and a recording to keep up with, and stole the list
 * back from anyone who had paged elsewhere.
 *
 * Requests are therefore identified by a nonce the caller bumps once per
 * click, not by the packet number, so asking for the same packet twice still
 * counts as two requests.
 */
export function open_action(
    request: { packet: number | null; nonce: number },
    handledNonce: number | null,
    view: { shown: boolean; contextMode: boolean; hasList: boolean }
): OpenAction {
    if (!view.shown) return { kind: 'nothing' };
    if (request.nonce === handledNonce) return { kind: 'nothing' };
    if (request.packet !== null) return { kind: 'focus', packet: request.packet };
    // Browsing. Reload only when there is nothing to show, or when what is on
    // screen is a focused window, which the browse view must not keep.
    if (view.contextMode || !view.hasList) return { kind: 'page' };
    return { kind: 'nothing' };
}
