import { describe, it, expect } from 'vitest';
import {
    apply_filters,
    direction_arrow,
    cells_present,
    DEFAULT_FILTERS,
    type PacketSummary,
} from './packets.svelte';

const packet = (over: Partial<PacketSummary> = {}): PacketSummary => ({
    packet_num: 1,
    timestamp: null,
    protocol: 'LTE RRC',
    channel: 'BcchDlSch',
    direction: 'downlink',
    message_type: 'SystemInformationBlockType1',
    payload_len: 15,
    parse_status: 'decoded',
    pci: 330,
    earfcn: 2125,
    sfn: 100,
    subfn: 3,
    ...over,
});

describe('apply_filters', () => {
    it('keeps everything when nothing is narrowed', () => {
        const packets = [packet(), packet({ packet_num: 2, protocol: 'LTE NAS' })];
        expect(apply_filters(packets, DEFAULT_FILTERS)).toHaveLength(2);
    });

    /**
     * Most of a capture carries no signalling at all, so hiding those by
     * default is the difference between a readable list and a wall of noise.
     */
    it('hides messages that carried no signalling by default', () => {
        const packets = [
            packet(),
            packet({ packet_num: 2, parse_status: 'not a signalling message' }),
        ];
        expect(apply_filters(packets, DEFAULT_FILTERS)).toHaveLength(1);
        expect(apply_filters(packets, { ...DEFAULT_FILTERS, decodedOnly: false })).toHaveLength(2);
    });

    it('separates the two protocols', () => {
        const packets = [packet(), packet({ packet_num: 2, protocol: 'LTE NAS' })];
        expect(apply_filters(packets, { ...DEFAULT_FILTERS, protocol: 'rrc' })).toHaveLength(1);
        expect(apply_filters(packets, { ...DEFAULT_FILTERS, protocol: 'nas' })[0].protocol).toBe(
            'LTE NAS'
        );
    });

    it('separates the two directions', () => {
        const packets = [packet(), packet({ packet_num: 2, direction: 'uplink' })];
        expect(apply_filters(packets, { ...DEFAULT_FILTERS, direction: 'uplink' })).toHaveLength(1);
    });

    it('searches the message name, channel and packet number', () => {
        const packets = [
            packet({ message_type: 'SecurityModeCommand' }),
            packet({ packet_num: 2, message_type: 'Paging' }),
        ];
        expect(apply_filters(packets, { ...DEFAULT_FILTERS, search: 'security' })).toHaveLength(1);
        expect(apply_filters(packets, { ...DEFAULT_FILTERS, search: 'bcch' })).toHaveLength(2);
        expect(apply_filters(packets, { ...DEFAULT_FILTERS, search: '2' })).toHaveLength(1);
    });

    it('is not confused by a missing message name', () => {
        const packets = [packet({ message_type: null, channel: null })];
        expect(apply_filters(packets, { ...DEFAULT_FILTERS, search: 'lte' })).toHaveLength(1);
    });
});

describe('direction_arrow', () => {
    it('points down for downlink and up for uplink', () => {
        expect(direction_arrow('downlink')).toBe('↓');
        expect(direction_arrow('uplink')).toBe('↑');
        expect(direction_arrow(null)).toBe('');
    });
});

describe('cells_present', () => {
    /**
     * A capture mixes messages from every cell in range. Seeing which cells are
     * there, and how much came from each, is often how an unexpected one gets
     * noticed at all.
     */
    it('counts packets per cell, busiest first', () => {
        const packets = [
            packet({ pci: 330 }),
            packet({ packet_num: 2, pci: 330 }),
            packet({ packet_num: 3, pci: 12 }),
        ];
        expect(cells_present(packets)).toEqual([
            { pci: 330, count: 2 },
            { pci: 12, count: 1 },
        ]);
    });

    it('ignores packets with no cell identity rather than inventing one', () => {
        expect(cells_present([packet({ pci: null })])).toEqual([]);
        expect(cells_present([])).toEqual([]);
    });
});

describe('search by cell', () => {
    it('finds packets by their cell identity', () => {
        const packets = [packet({ pci: 330 }), packet({ packet_num: 2, pci: 12 })];
        expect(apply_filters(packets, { ...DEFAULT_FILTERS, search: 'pci330' })).toHaveLength(1);
    });
});
