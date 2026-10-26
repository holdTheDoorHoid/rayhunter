import { describe, it, expect } from 'vitest';
import { render } from 'svelte/server';
import CellInfoView from './CellInfoView.svelte';
import type { CellInfo, SignalMeasurements } from '../cellInfo';

function info(signal: SignalMeasurements | null): CellInfo {
    return {
        serving: {
            pci: 219,
            earfcn: 3350,
            band: 7,
            signal,
            identity: null,
            search_threshold: null,
            timing_advance: null,
            last_seen: new Date().toISOString(),
        },
        has_data: true,
        neighbors: [],
        history: [],
        health: { messages_seen: 2, messages_skipped: 0, last_message: new Date().toISOString() },
        sim_health: {
            verdict: 'searching',
            data_interface: null,
            nas_recent: false,
            last_nas_message: null,
            serving_cell: true,
            silent_for_minutes: null,
        },
        timing_advance: { state: 'unsupported' },
    };
}

describe('Cell Site with an RRC-only serving cell', () => {
    it('renders the cell and missing signal honestly without throwing', () => {
        const { body } = render(CellInfoView, { props: { info: info(null), recording: true } });
        expect(body).toContain('EARFCN 3350');
        expect(body).toContain('not captured');
        expect(body).toContain('observed');
        expect(body).toContain('Signal measurements have not been captured');
        expect(body).not.toContain('0.0 dBm');
        expect(body).not.toContain('Waiting for cell measurements');
    });

    it('still renders measured signal when ML1 is available', () => {
        const signal = {
            rsrp_dbm: -85,
            rsrq_db: -10,
            rssi_dbm: -60,
            avg_rsrp_dbm: -86,
            avg_rsrq_db: null,
        };
        const { body } = render(CellInfoView, { props: { info: info(signal), recording: true } });
        expect(body).toContain('-85.0 dBm');
        expect(body).toContain('-10.0 dB');
        expect(body).not.toContain('Signal measurements have not been captured');
    });
});
