import { get_report, type AnalysisReport } from './analysis.svelte';
import { AnalysisStatus, type AnalysisManager } from './analysisManager.svelte';
import { GpsMode } from './utils.svelte';

interface JsonManifest {
    entries: JsonManifestEntry[];
    current_entry: JsonManifestEntry | null;
}

interface JsonManifestEntry {
    name: string;
    start_time: string;
    last_message_time: string;
    qmdl_size_bytes: number;
    stop_reason: string | null;
    upload_time: string | null;
    gps_mode: GpsMode | null;
    display_name: string | null;
    notes: string | null;
}

export class Manifest {
    public entries: ManifestEntry[] = [];
    public current_entry: ManifestEntry | undefined;

    constructor(json: JsonManifest) {
        for (const entry of json.entries) {
            this.entries.push(new ManifestEntry(entry));
        }
        if (json.current_entry !== null) {
            this.current_entry = new ManifestEntry(json['current_entry']);
        }

        // sort entries in reverse chronological order
        this.entries.reverse();
    }

    async set_analysis_status(manager: AnalysisManager) {
        for (const entry of this.entries) {
            entry.analysis_status = manager.status.get(entry.name);
            entry.analysis_report = manager.reports.get(entry.name);
        }

        if (this.current_entry) {
            try {
                this.current_entry.analysis_report = await get_report(this.current_entry.name);
            } catch (err) {
                this.current_entry.analysis_report = `Err: failed to get analysis report: ${err}`;
            }

            // the current entry should always be considered "finished", as its
            // analysis report is always available
            this.current_entry.analysis_status = AnalysisStatus.Finished;
        }
    }
}

/**
 * What Rayhunter knew about the device when a recording was made, as saved
 * in `<name>-meta.json` beside it. Every field but the version and name is
 * best effort and may be missing.
 */
export type RecordingSidecar = {
    sidecar_version: number;
    recording: string;
    software: {
        rayhunter_version: string;
        system_os: string;
        arch: string;
        kernel?: string;
    };
    hardware: {
        device: string;
        model?: string;
        hardware_version?: string;
        soc?: string;
        firmware_build?: string;
    };
    home_plmn: string[];
    clock: {
        system_time_at_start?: string;
        offset_seconds_at_start: number;
        corrected_time_at_start?: string;
        uptime_seconds_at_start?: number;
        system_time_at_end?: string;
        offset_seconds_at_end?: number;
        uptime_seconds_at_end?: number;
        offset_changed_during_recording?: boolean;
    };
    resources: {
        storage_path?: string;
        disk_total_bytes?: number;
        disk_available_bytes?: number;
        memory_total_kb?: number;
        memory_available_kb?: number;
    };
    wifi: {
        client_enabled: boolean;
        client_state: string;
        connected_network?: string;
    } | null;
    redacted_fields?: string[];
};

export class ManifestEntry {
    public name = $state('');
    public start_time: Date;
    public last_message_time: Date | undefined = $state(undefined);
    public qmdl_size_bytes = $state(0);
    public analysis_size_bytes = $state(0);
    public analysis_status: AnalysisStatus | undefined = $state(undefined);
    public analysis_report: AnalysisReport | string | undefined = $state(undefined);
    public stop_reason: string | undefined = $state(undefined);
    public upload_time: Date | undefined = $state(undefined);
    public gps_mode: GpsMode | undefined = $state(undefined);
    /** A name chosen by the person recording, shown instead of the timestamp. */
    public display_name: string | null = $state(null);
    /** Free text about the circumstances of the recording. */
    public notes: string | null = $state(null);

    constructor(json: JsonManifestEntry) {
        this.name = json.name;
        this.qmdl_size_bytes = json.qmdl_size_bytes;
        this.start_time = new Date(json.start_time);
        if (json.last_message_time) {
            this.last_message_time = new Date(json.last_message_time);
        }
        if (json.stop_reason) {
            this.stop_reason = json.stop_reason;
        }
        if (json.upload_time) {
            this.upload_time = new Date(json.upload_time);
        }
        if (json.gps_mode !== null) {
            this.gps_mode = json.gps_mode;
        }
        // A daemon older than this UI sends neither.
        this.display_name = json.display_name ?? null;
        this.notes = json.notes ?? null;
    }

    /** What to call this recording on screen. */
    get_label(): string {
        return this.display_name ?? this.name;
    }

    get_readable_qmdl_size(): string {
        if (this.qmdl_size_bytes === 0) return '0 Bytes';
        const k = 1024;
        const dm = 2;
        const sizes = ['Bytes', 'KB', 'MB', 'GB', 'TB', 'PB', 'EB', 'ZB', 'YB'];
        const i = Math.floor(Math.log(this.qmdl_size_bytes) / Math.log(k));
        return `${Number.parseFloat((this.qmdl_size_bytes / k ** i).toFixed(dm))} ${sizes[i]}`;
    }

    get_num_warnings(): number | undefined {
        if (this.analysis_report === undefined || typeof this.analysis_report === 'string') {
            return undefined;
        }
        return this.analysis_report.statistics.num_warnings;
    }

    get_pcap_url(): string {
        return `/api/pcap/${this.name}.pcapng`;
    }

    get_qmdl_url(): string {
        return `/api/qmdl/${this.name}.qmdl`;
    }

    get_zip_url(): string {
        return `/api/zip/${this.name}.zip`;
    }

    /**
     * A bundle meant to be shared: the device's own identifiers removed from
     * the capture, and the raw recording left out, since nothing removes them
     * from that. It reports what it took out rather than claiming to be clean.
     */
    get_redacted_zip_url(): string {
        return `/api/zip/${this.name}.zip?redact=1`;
    }

    get_analysis_report_url(): string {
        return `/api/analysis-report/${this.name}`;
    }

    /**
     * The device details saved beside the recording. 404 for recordings
     * made before Rayhunter saved them.
     */
    get_metadata_url(): string {
        return `/api/recording-metadata/${this.name}`;
    }

    get_delete_url(): string {
        return `/api/delete-recording/${this.name}`;
    }

    get_reanalyze_url(): string {
        return `/api/analysis/${this.name}`;
    }
}
