/**
 * Starting a new recording automatically, on size or on elapsed time.
 *
 * A capture left running produces one file that grows until the disk fills.
 * Splitting it keeps any single recording small enough to download over the
 * device's own wifi, and means each piece is analysed and readable while
 * capture carries on rather than only once it is stopped.
 */

/** Minutes offered in the dropdown. Anything else is entered by hand. */
export const TIME_PRESETS_MINUTES = [15, 30, 60, 120, 360, 720, 1440];

/** Megabytes offered in the dropdown. Anything else is entered by hand. */
export const SIZE_PRESETS_MB = [5, 10, 25, 50, 100];

/**
 * Below this, rotation costs more than it gives.
 *
 * Closing a recording queues it for analysis, and these devices have one core
 * that is also keeping up with the radio. Rotating every few seconds would
 * leave the analyser permanently behind, which is a detector that has stopped
 * detecting.
 */
export const MIN_MINUTES = 1;
export const MIN_SIZE_MB = 1;

/** A duration in words, for a limit expressed in minutes. */
export function format_minutes(minutes: number): string {
    if (minutes < 60) return `${minutes} minute${minutes === 1 ? '' : 's'}`;
    if (minutes % 1440 === 0) {
        const days = minutes / 1440;
        return `${days} day${days === 1 ? '' : 's'}`;
    }
    if (minutes % 60 === 0) {
        const hours = minutes / 60;
        return `${hours} hour${hours === 1 ? '' : 's'}`;
    }
    const hours = Math.floor(minutes / 60);
    const rest = minutes % 60;
    return `${hours} hour${hours === 1 ? '' : 's'} ${rest} minute${rest === 1 ? '' : 's'}`;
}

/**
 * The same duration phrased to follow the word "every".
 *
 * "Every 1 hour" is what you get from counting units, and it is not what
 * anybody says. A quantity of one is left implied.
 */
export function format_interval(minutes: number): string {
    if (minutes === 60) return 'hour';
    if (minutes === 1440) return 'day';
    if (minutes === 1) return 'minute';
    return format_minutes(minutes);
}

/**
 * What the two limits add up to, said plainly.
 *
 * The pair interacts, and reading two separate fields does not make the
 * combined behaviour obvious. Stating it in one sentence is the difference
 * between a setting somebody can predict and one they have to test.
 */
export function rotation_summary(sizeMb: number | null, minutes: number | null): string {
    const size = sizeMb && sizeMb > 0 ? sizeMb : null;
    const time = minutes && minutes > 0 ? minutes : null;

    if (!size && !time) {
        return 'One recording keeps running until you stop it, or until the device runs out of space.';
    }
    if (size && !time) {
        return `A new recording starts each time the current one reaches ${size} MB.`;
    }
    if (!size && time) {
        return `A new recording starts every ${format_interval(time!)}.`;
    }
    return `A new recording starts every ${format_interval(time!)}, or sooner if the current one reaches ${size} MB.`;
}

/**
 * Roughly how often rotation will happen, for warning about aggressive values.
 *
 * Returns null when nothing is set. Size cannot be turned into a rate without
 * knowing the capture rate, so only the time limit is judged here.
 */
export function rotation_warning(sizeMb: number | null, minutes: number | null): string | null {
    if (minutes !== null && minutes > 0 && minutes < 5) {
        return 'Rotating this often leaves the device little time to analyse each recording before the next one arrives.';
    }
    if (sizeMb !== null && sizeMb > 0 && sizeMb < 2) {
        return 'A limit this small produces a great many recordings, which is slow to browse and slow to analyse.';
    }
    return null;
}
