export interface SystemStats {
    disk_stats: DiskStats;
    memory_stats: MemoryStats;
    runtime_metadata: RuntimeMetadata;
    battery_status?: BatteryStatus;
    health?: HealthStats;
}

export interface RuntimeMetadata {
    rayhunter_version: string;
    system_os: string;
    arch: string;
}

export interface DiskStats {
    partition: string;
    total_size: string;
    used_size: string;
    available_size: string;
    used_percent: string;
    mounted_on: string;
    available_bytes?: number;
}

export interface MemoryStats {
    total: string;
    used: string;
    free: string;
}

export interface BatteryStatus {
    level: number;
    is_plugged_in: boolean;
}

/** Load, uptime and temperature, when the platform reports them. */
export interface HealthStats {
    uptime_secs: number;
    /** One, five and fifteen minute load averages. */
    load_avg: [number, number, number];
    /** Cores the load is spread across. One on these devices. */
    cpu_count: number;
    cpu_temp_c?: number;
    radio_temp_c?: number;
}

/**
 * Load relative to the number of cores.
 *
 * A load of 1.4 sounds mild until you know the device has a single core, at
 * which point it means work is queuing. Dividing by the core count is what
 * makes the number comparable.
 */
export function load_per_core(health: HealthStats): number {
    return health.load_avg[0] / Math.max(1, health.cpu_count);
}

export type LoadState = 'idle' | 'busy' | 'saturated' | 'overloaded';

export function load_state(health: HealthStats): LoadState {
    const per = load_per_core(health);
    if (per < 0.7) return 'idle';
    if (per < 1.0) return 'busy';
    if (per < 2.0) return 'saturated';
    return 'overloaded';
}

/** Uptime in words, e.g. "3 hours 12 minutes". */
export function format_uptime(seconds: number): string {
    if (seconds < 60) return `${Math.floor(seconds)} seconds`;
    const days = Math.floor(seconds / 86400);
    const hours = Math.floor((seconds % 86400) / 3600);
    const minutes = Math.floor((seconds % 3600) / 60);
    if (days > 0)
        return `${days} day${days === 1 ? '' : 's'} ${hours} hour${hours === 1 ? '' : 's'}`;
    if (hours > 0)
        return `${hours} hour${hours === 1 ? '' : 's'} ${minutes} minute${minutes === 1 ? '' : 's'}`;
    return `${minutes} minute${minutes === 1 ? '' : 's'}`;
}

/**
 * How long the free space will last at the rate a recording is growing.
 *
 * Returns null until there are two samples far enough apart to mean anything,
 * and when the recording is not growing at all. Guessing from a single reading
 * would produce a confident number with nothing behind it.
 */
export function hours_until_full(
    bytesPerSecond: number,
    availableBytes: number | undefined
): number | null {
    if (!availableBytes || bytesPerSecond <= 0) return null;
    return availableBytes / bytesPerSecond / 3600;
}

/** A duration in words, for the storage estimate. */
export function format_duration_hours(hours: number): string {
    if (hours < 1) return `${Math.max(1, Math.round(hours * 60))} minutes`;
    if (hours < 48) return `${Math.round(hours)} hours`;
    return `${Math.round(hours / 24)} days`;
}
