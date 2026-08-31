/**
 * Whether the expandable explanations are shown.
 *
 * The explanations exist because a lot of what Rayhunter reports is
 * meaningless without them: RSRP, tracking areas, integrity algorithms, what a
 * heuristic actually watches for. Somebody meeting the device for the first
 * time needs all of it. Somebody who has read it once needs none of it, and by
 * then it is just clutter between them and the numbers.
 *
 * So it is a preference rather than a fixed choice, and it defaults to on: a
 * new device should explain itself, and hiding help should be a decision
 * somebody makes rather than one made for them.
 *
 * Kept in browser storage for the same reasons as the theme. Saving the device
 * config restarts Rayhunter, and interrupting a recording to hide some help
 * text would be a poor trade. It is also a per reader choice: the person
 * setting a device up and the person reading it later are often not the same.
 */

export const HELP_STORAGE_KEY = 'rayhunter-show-help';

/**
 * Interpret a stored value.
 *
 * Only an explicit "false" hides the help. Absent means never chosen, and
 * anything else means the value did not come from here, and neither is a
 * reason to hide it.
 */
export function help_shown_from_stored(stored: string | null): boolean {
    return stored !== 'false';
}

function load(): boolean {
    try {
        return help_shown_from_stored(localStorage.getItem(HELP_STORAGE_KEY));
    } catch {
        // Private windows and blocked site data both throw. Showing the help is
        // the safer default when the preference cannot be read.
        return true;
    }
}

class HelpVisibility {
    shown = $state(true);

    init() {
        this.shown = load();
    }

    set(shown: boolean) {
        this.shown = shown;
        try {
            localStorage.setItem(HELP_STORAGE_KEY, String(shown));
        } catch {
            // Not being able to remember the choice is not a reason to ignore
            // it for this session.
        }
    }
}

export const help = new HelpVisibility();
