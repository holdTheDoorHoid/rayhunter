/**
 * Plain language explanations of each detection heuristic.
 *
 * Written for someone with no background in cellular networks, because the
 * settings page is where people decide whether to switch a detector off, and
 * that decision is not safe to make from a name like "NAS Null Cipher". Each
 * entry carries a one sentence summary that is always on screen, plus a longer
 * explanation of what the test looks for, why it matters for privacy, and when
 * it can fire harmlessly.
 *
 * The longer text mirrors doc/heuristics.md, condensed and de-jargonised. Keep
 * the two in step when either changes.
 */

import type { AnalyzerConfig } from './utils.svelte';

/**
 * Derived from the config rather than written out, so adding an analyzer
 * without an explanation here becomes a type error rather than a setting that
 * silently ships with no description.
 */
export type AnalyzerKey = keyof AnalyzerConfig;

export interface HeuristicInfo {
    key: AnalyzerKey;
    /** Short, human name. Avoids acronyms where a plain word will do. */
    title: string;
    /** One sentence, always visible under the title. */
    summary: string;
    /** What the test actually looks for. */
    detects: string;
    /** Why that matters to someone worried about being tracked or listened to. */
    matters: string;
    /** When it can fire without anything being wrong. */
    noise?: string;
    /** Shown as a small tag when the detector is unusually noisy or advisory. */
    tag?: 'noisy' | 'informational';
}

export const HEURISTICS: HeuristicInfo[] = [
    {
        key: 'imsi_requested',
        title: 'Identity requested without proof of identity',
        summary:
            'Watches for a tower that asks your phone who it is, never proves it is a real network, then drops the connection.',
        detects:
            'Your phone normally identifies itself with a temporary number that changes regularly, so nobody can follow you around by name. Occasionally a genuine network needs your permanent number, most often the first time you connect after being switched off for a long while. What this test looks for is the whole suspicious pattern rather than the request alone: a tower asks for your permanent identity, never authenticates itself the way a real network must, and then tells your phone to go away.',
        matters:
            'That permanent number identifies you personally, and it does not change. Anyone who collects it can confirm you were in a particular place at a particular time. The pattern above is precisely how the surveillance devices sold to law enforcement, often called IMSI catchers or stingrays, capture identities from a crowd. This is the most direct evidence Rayhunter can give you that your identity was taken, so switching it off removes your clearest warning.',
        noise: 'It sometimes fires on aircraft coming in to land, likely because the phone has been disconnected for a while and is passing over towers that cannot reach your home network.',
    },
    {
        key: 'connection_redirect_2g_downgrade',
        title: 'Pushed down to a 2G network',
        summary: 'Watches for a tower that hands your phone off onto an old 2G network.',
        detects:
            'A tower can tell your phone to release its connection and move to a different network. This test notices when the destination is 2G, the generation of mobile network designed in the 1990s.',
        matters:
            'The encryption used on 2G is broken and has been for years. Surveillance equipment often forces phones down onto 2G for exactly that reason, because once you are there your calls and messages can be read and even altered as they pass through. A modern network has no good reason to push a working 4G connection down to 2G.',
        noise: 'Worth leaving on even in places where 2G is still ordinary. The concern is being moved there by a tower, not 2G existing.',
    },
    {
        key: 'lte_sib6_and_7_downgrade',
        title: 'Old networks advertised as better than nearby 4G',
        summary:
            'Watches for a tower that advertises 2G or 3G networks as higher priority choices than the 4G towers around it.',
        detects:
            'Towers constantly broadcast a list of other networks nearby, each with a priority telling phones which to prefer if they need to move. This test looks for lists that rank old 2G or 3G networks above nearby 4G.',
        matters:
            'Those broadcasts are not encrypted and carry no signature, so anyone with the right equipment can transmit their own. Advertising weak networks as the preferred choice is a quiet way to steer phones onto a network where they can be intercepted, without ever touching the phone directly. A properly configured tower always ranks its 4G neighbours highest.',
        noise: 'Earlier versions of this test raised many false alarms. The current version is considerably stricter.',
    },
    {
        key: 'null_cipher',
        title: 'Encryption switched off by the tower',
        summary: 'Watches for a tower asking your phone to communicate with no encryption at all.',
        detects:
            'Before your phone exchanges anything meaningful with a tower, the two agree how to encrypt it. This test catches the tower proposing an option that means no encryption whatsoever.',
        matters:
            'With encryption off, everything travelling between your phone and that tower is readable by anyone within range who is listening. Real networks essentially never do this outside a test lab. Fake towers do it routinely, because they do not hold the cryptographic keys that real encryption would require, and turning it off is the simplest way around that.',
    },
    {
        key: 'nas_null_cipher',
        title: 'Encryption switched off by the core network',
        summary:
            'Watches for the operator core network turning encryption off after your phone has already been verified.',
        detects:
            'The same absence of encryption as the previous test, but requested at a deeper level and after your phone has successfully proved its identity to the network.',
        matters:
            'This is the more serious of the two. Getting to this point means whoever is doing it holds genuine key material for your SIM, which is not something an ordinary fake tower can obtain. It can point to cooperation from the operator itself, or to an attack on the signalling network that carriers use to pass subscriber information between each other.',
        noise: 'A carrier that is genuinely misconfigured, or operating where encryption is restricted by law, could also produce this. Both are rare.',
    },
    {
        key: 'incomplete_sib',
        title: 'Tower broadcasting only a fragment of its details',
        summary:
            'Watches for towers that broadcast far less information about themselves than a real one does.',
        detects:
            'A genuine tower continuously broadcasts a series of information blocks describing itself, its neighbours and how to use it. This test checks whether the first block promises the others, and whether they are actually there.',
        matters:
            'Fake towers commonly send only the first block or two. The rest takes effort and brings whoever is operating it no benefit, since they only need your phone to connect briefly. On its own this can simply be a badly configured tower. Alongside an identity request it becomes a strong signal that the tower is not what it claims to be.',
    },
    {
        key: 'diagnostic_analyzer',
        title: 'Connection diary',
        summary:
            'Records when your phone joins and leaves each tower. This produces notes rather than alarms.',
        detects: 'Ordinary connection events, written into the recording as informational entries.',
        matters:
            'Useful when looking back over a recording to understand what your phone was doing around the time something else fired. Its own entries are not warnings and can be ignored until a low, medium or high warning appears alongside them.',
        tag: 'informational',
    },
    {
        key: 'test_analyzer',
        title: 'Alert on every tower, for testing',
        summary:
            'Raises a warning every single time any tower is seen, purely to confirm your Rayhunter is working.',
        detects:
            'Every tower your phone encounters, with no judgement about whether it is suspicious.',
        matters:
            'Switch this on briefly if you want proof the device is detecting anything at all. If it produces warnings, Rayhunter is working. Leave it off the rest of the time, because a constant stream of alerts will bury the real ones.',
        tag: 'noisy',
    },
];

/**
 * Lookup by key. Also the value the exhaustiveness test checks against, so a
 * new analyzer cannot reach the settings page unexplained.
 */
export const HEURISTICS_BY_KEY: Record<AnalyzerKey, HeuristicInfo> = Object.fromEntries(
    HEURISTICS.map((h) => [h.key, h])
) as Record<AnalyzerKey, HeuristicInfo>;
