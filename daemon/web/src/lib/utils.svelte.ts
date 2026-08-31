import { add_error } from './action_errors.svelte';
import { Manifest } from './manifest.svelte';
import type { SystemStats } from './systemStats';

export interface AnalyzerConfig {
    imsi_requested: boolean;
    connection_redirect_2g_downgrade: boolean;
    lte_sib6_and_7_downgrade: boolean;
    null_cipher: boolean;
    nas_null_cipher: boolean;
    incomplete_sib: boolean;
    test_analyzer: boolean;
    diagnostic_analyzer: boolean;
}

export enum enabled_notifications {
    Warning = 'Warning',
    LowBattery = 'LowBattery',
    Update = 'Update',
}

export interface WebdavConfig {
    url: string;
    username: string | null;
    password: string | null;
    upload_timeout_secs: number;
    poll_interval_secs: number;
    min_age_secs: number;
    delete_on_upload: boolean;
}

export enum GpsMode {
    Disabled = 0,
    Fixed = 1,
    Api = 2,
}

export function gps_mode_label(mode: GpsMode | undefined | null): string {
    switch (mode) {
        case GpsMode.Fixed:
            return 'Fixed coordinates';
        case GpsMode.Api:
            return 'API endpoint';
        default:
            return 'Disabled';
    }
}

/**
 * Per-state color overrides for the device's own display, as `#rrggbb` strings.
 * A `null` field means "use Rayhunter's built-in color for this state".
 */
export interface DisplayColors {
    paused: string | null;
    recording: string | null;
    warning_low: string | null;
    warning_medium: string | null;
    warning_high: string | null;
}

export type DisplayColorKey = keyof DisplayColors;

/**
 * The built-in color for each display state, used both as the starting value
 * for the color pickers and to preview what "unset" looks like. `recording`
 * depends on whether colorblind mode is enabled, so it is resolved separately
 * by `default_recording_color`.
 */
export const DISPLAY_COLOR_DEFAULTS: Record<DisplayColorKey, string> = {
    paused: '#ffffff',
    recording: '#00ff00',
    warning_low: '#ffff00',
    warning_medium: '#ffa500',
    warning_high: '#ff0000',
};

/** The recording color Rayhunter uses when no override is set. */
export function default_recording_color(colorblind_mode: boolean): string {
    return colorblind_mode ? '#0000ff' : DISPLAY_COLOR_DEFAULTS.recording;
}

/**
 * Filenames of the GIFs uploaded per display state, used when ui_level is
 * Custom GIF (5). `null` means that state falls back to its colored line.
 */
export interface DisplayGifs {
    paused: string | null;
    recording: string | null;
    warning_low: string | null;
    warning_medium: string | null;
    warning_high: string | null;
}

/** Largest GIF the device accepts. Must match MAX_GIF_BYTES in the daemon. */
export const MAX_GIF_BYTES = 2 * 1024 * 1024;

/** The device screen is square and small; GIFs are scaled to fit this. */
export const DEVICE_SCREEN_PX = 128;

export async function set_display_gif(state: DisplayColorKey, file: File): Promise<void> {
    const response = await fetch(`/api/display-gif/${state}`, {
        method: 'POST',
        headers: { 'Content-Type': 'image/gif' },
        body: file,
    });
    if (!response.ok) {
        throw new Error(await response.text());
    }
}

export async function delete_display_gif(state: DisplayColorKey): Promise<void> {
    const response = await fetch(`/api/display-gif/${state}/delete`, { method: 'POST' });
    if (!response.ok) {
        throw new Error(await response.text());
    }
}

export interface Config {
    device: string;
    ui_level: number;
    colorblind_mode: boolean;
    demo_mode: boolean;
    display_colors: DisplayColors;
    status_bar_height: number | null;
    display_gifs: DisplayGifs;
    key_input_mode: number;
    ntfy_url: string | null;
    enabled_notifications: enabled_notifications[];
    auto_check_updates: boolean;
    analyzers: AnalyzerConfig;
    min_space_to_start_recording_mb: number;
    min_space_to_continue_recording_mb: number;
    /** Start a new recording at this size. Null or 0 keeps one running. */
    max_recording_size_mb: number | null;
    /** Start a new recording after this long. Null or 0 keeps one running. */
    max_recording_minutes: number | null;
    wifi_ssid: string | null;
    wifi_password: string | null;
    wifi_security: 'wpa_psk' | 'sae' | null;
    wifi_enabled: boolean;
    dns_servers: string[] | null;
    firewall_restrict_outbound: boolean;
    firewall_allowed_ports: number[] | null;
    webdav: WebdavConfig;
    gps_mode: GpsMode;
    gps_fixed_latitude: number | null;
    gps_fixed_longitude: number | null;
}

export interface WifiStatus {
    state: string;
    ssid?: string;
    ip?: string;
    error?: string;
}

export interface WifiNetwork {
    ssid: string;
    signal_dbm: number;
    security: string;
}

export async function get_wifi_status(): Promise<WifiStatus> {
    return JSON.parse(await req('GET', '/api/wifi-status'));
}

export async function scan_wifi_networks(): Promise<WifiNetwork[]> {
    return JSON.parse(await req('POST', '/api/wifi-scan'));
}

export async function req(method: string, url: string, json_body?: unknown): Promise<string> {
    const options: RequestInit = { method };
    if (json_body !== undefined) {
        options.body = JSON.stringify(json_body);
        options.headers = { 'Content-Type': 'application/json' };
    }
    const response = await fetch(url, options);
    const responseBody = await response.text();
    if (response.status >= 200 && response.status < 300) {
        return responseBody;
    } else {
        throw new Error(responseBody);
    }
}

// A wrapper around req that reports errors to the UI
export async function user_action_req(
    method: string,
    url: string,
    error_msg: string,
    json_body?: unknown
): Promise<string | undefined> {
    try {
        return await req(method, url, json_body);
    } catch (error) {
        if (error instanceof Error) {
            add_error(error, error_msg);
        }
        return undefined;
    }
}

export async function get_manifest(): Promise<Manifest> {
    const manifest_json = JSON.parse(await req('GET', '/api/qmdl-manifest'));
    return new Manifest(manifest_json);
}

export async function get_system_stats(): Promise<SystemStats> {
    return JSON.parse(await req('GET', '/api/system-stats'));
}

export async function get_logs(): Promise<string> {
    return await req('GET', '/api/log');
}

export async function get_config(): Promise<Config> {
    return JSON.parse(await req('GET', '/api/config'));
}

export async function set_config(config: Config): Promise<void> {
    const response = await fetch('/api/config', {
        method: 'POST',
        headers: {
            'Content-Type': 'application/json',
        },
        body: JSON.stringify(config),
    });

    if (!response.ok) {
        const error = await response.text();
        throw new Error(error);
    }
}

/**
 * Inject a synthetic, clearly labelled warning for demonstrating Rayhunter.
 * Refused by the daemon unless demo mode is enabled in the config.
 */
export async function trigger_demo_warning(): Promise<string> {
    const response = await fetch('/api/demo-warning', { method: 'POST' });
    const body = await response.text();
    if (!response.ok) throw new Error(body);
    return body;
}

export async function test_notification(): Promise<void> {
    const response = await fetch('/api/test-notification', {
        method: 'POST',
    });

    if (!response.ok) {
        const error = await response.text();
        throw new Error(error);
    }
}

export interface TimeResponse {
    system_time: string;
    adjusted_time: string;
    offset_seconds: number;
}

export interface UpdateStatus {
    current_version: string;
    latest_version?: string | null;
    latest_release_url?: string | null;
    update_available: boolean;
    last_checked?: string | null;
    last_error?: string | null;
}

export async function get_daemon_time(): Promise<TimeResponse> {
    return JSON.parse(await req('GET', '/api/time'));
}

export async function get_update_status(): Promise<UpdateStatus> {
    return JSON.parse(await req('GET', '/api/update-status'));
}

export interface GpsData {
    latitude: number;
    longitude: number;
    /** Unix timestamp in seconds (0 = fixed/no real time). */
    timestamp: number;
}

export async function get_gps(): Promise<GpsData | null> {
    const response = await fetch('/api/gps', { cache: 'no-store' });
    if (response.status === 404) {
        return null;
    }
    if (response.status >= 200 && response.status < 300) {
        return response.json();
    }
    throw new Error(await response.text());
}
