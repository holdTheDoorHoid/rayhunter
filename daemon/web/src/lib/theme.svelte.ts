/**
 * Light and dark appearance for the web UI.
 *
 * The preference is kept in the browser rather than in the device config on
 * purpose. Saving the device config restarts Rayhunter, and interrupting a
 * recording to change a colour scheme would be a poor trade. It is also a
 * per viewer choice: the same device may be opened from a phone set to dark
 * and a laptop set to light, and each should look right.
 */

export type ThemePreference = 'system' | 'light' | 'dark';

const STORAGE_KEY = 'rayhunter-theme';

export function is_theme_preference(value: unknown): value is ThemePreference {
    return value === 'system' || value === 'light' || value === 'dark';
}

/** Resolve a preference into the appearance actually shown. */
export function resolve_theme(preference: ThemePreference, system_prefers_dark: boolean): boolean {
    if (preference === 'dark') return true;
    if (preference === 'light') return false;
    return system_prefers_dark;
}

export function load_preference(): ThemePreference {
    try {
        const stored = localStorage.getItem(STORAGE_KEY);
        if (is_theme_preference(stored)) return stored;
    } catch {
        // Private windows and blocked site data both throw here. Falling back
        // to the system setting is the right answer either way.
    }
    return 'system';
}

function system_prefers_dark(): boolean {
    return (
        typeof window !== 'undefined' &&
        window.matchMedia?.('(prefers-color-scheme: dark)').matches === true
    );
}

function apply(dark: boolean) {
    document.documentElement.classList.toggle('dark', dark);
    // Tells the browser to render form controls and scrollbars to match.
    document.documentElement.style.colorScheme = dark ? 'dark' : 'light';
}

class Theme {
    preference = $state<ThemePreference>('system');
    /** Whether dark is currently being shown, after resolving "system". */
    is_dark = $state(false);

    /** Wire up on first mount. Safe to call more than once. */
    init() {
        this.preference = load_preference();
        this.refresh();

        // Follow the system while the preference is "system", so the page
        // changes with the phone at sunset without needing a reload.
        window
            .matchMedia?.('(prefers-color-scheme: dark)')
            .addEventListener?.('change', () => this.refresh());
    }

    refresh() {
        this.is_dark = resolve_theme(this.preference, system_prefers_dark());
        apply(this.is_dark);
    }

    set(preference: ThemePreference) {
        this.preference = preference;
        try {
            localStorage.setItem(STORAGE_KEY, preference);
        } catch {
            // Not being able to remember the choice is not a reason to ignore it
            // for this session.
        }
        this.refresh();
    }
}

export const theme = new Theme();
