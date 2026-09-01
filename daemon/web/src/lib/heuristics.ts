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
        key: 'lpp_location_request',
        title: 'Location asked for by the network',
        summary:
            'Watches for the network asking your device to measure and report exactly where it is.',
        detects:
            'Mobile networks have a dedicated protocol, LPP, for asking a phone to report its own position: satellite readings, measurements of nearby towers, or a computed location. This test watches for those requests arriving, and for your device answering them. The first request in an exchange raises a low warning; the routine technical chatter around it, such as the network asking what your device can measure, is recorded quietly as informational notes.',
        matters:
            'This is a more precise capability than anything else a tower can do: it does not estimate where you are, it asks your device to say. It exists for emergencies, so that a 911 call can be found, but the same machinery has been used by surveillance equipment and by carriers to track people continuously without anything visible happening on the device. A warning here means your position was asked for by name, and is worth reading together with whatever else was happening at the time.',
        noise: 'Fires legitimately during emergency calls, and on some carriers that use network location for lawful services. A hotspot sitting on a desk should see this rarely, so an unexplained warning is worth attention.',
    },
    {
        key: 'lpp_location_tracking',
        title: 'Continuous location tracking',
        summary:
            'Looks more closely at those location requests, and raises the alarm when the network asks to be told your position over and over.',
        detects:
            'This is the deeper look at the same location messages as the check above. It reads how the network asked: which method it wants your device to use, from a precise satellite fix down to a rough cell estimate, and, most importantly, whether it asked for your position once or asked for it to be reported continuously on a timer. A repeated, on-a-timer request is flagged more seriously than a single one.',
        matters:
            'The difference between being located once and being followed is the whole point. A single location request during an emergency call is ordinary; a standing request that reports where you are every few seconds, for as long as it lasts, is what continuous tracking actually looks like. Reading that distinction is what separates a routine locate from surveillance.',
        noise: 'Same legitimate causes as the check above. This one reads more of each message, so it does a little more work; on a device very short on memory you can turn it off and keep the basic version. Leaving both on is fine.',
    },
    {
        key: 'rrlp_location_request',
        title: 'Location asked for on 2G (older networks)',
        summary:
            'The same idea as the location checks above, but for the older 2G network that phones fall back to.',
        detects:
            "2G networks have their own, older way of asking a phone for its position, from before the one modern networks use. This watches 2G signalling for that request and for your device's answer.",
        matters:
            'Being pushed onto 2G is itself a known surveillance move, because its protections are weak, and once there this older location request is how a device can be pinpointed. Watching for it means a switch to 2G followed by a location request does not go unnoticed. It is the 2G companion to the checks above, so the same concern applies wherever your phone still uses 2G.',
        noise: 'Legitimate on emergency calls on 2G. On a device that never touches 2G this will simply stay quiet.',
    },
    {
        key: 'timing_advance',
        title: 'A tower that seems to have moved',
        summary:
            'Watches whether a cell keeps answering from the same distance, because real towers do not move.',
        detects:
            'Every time your device starts talking to a tower, the tower tells it how far away it is, so the device can time its transmissions to arrive in the right slot. That figure is a rough distance. This check remembers the distance each cell reported and notices when the same cell suddenly answers from somewhere markedly different, more than about a kilometre away from where it was.',
        matters:
            'A fake base station attracts devices by copying a real tower\u2019s identifiers, so on paper it looks like a cell your phone already trusts. What it cannot copy is where that tower physically stands. So one cell identity answering from two different places is a strong sign that two different transmitters are using the same name, which is exactly what impersonating a tower looks like from the inside.',
        noise: 'Moving sets this off honestly: if you carry the device any real distance, the towers around you genuinely change distance, and this will say so. It is reported gently for that reason. Many devices never report this measurement at all, including the Orbic, and on those this check stays silent rather than guessing.',
    },
    {
        key: 'diagnostic_analyzer',
        title: 'Identity exposure diary',
        summary:
            'Keeps notes on the moments your permanent identity could have been asked for or exposed. Notes, not alarms.',
        detects:
            'The messages that can lead to your permanent identity being sent: a tower asking your device to identify itself, a tower refusing service for reasons that make a device reintroduce itself from scratch, and a tower ordering a disconnect. Each is written into the recording as an informational note saying which message it was and why the tower said it happened.',
        matters:
            'Every one of these messages also occurs on real networks for ordinary reasons, so a note here proves nothing on its own. Their value is context: when another detector raises a warning, the notes show what the tower was doing and claiming at that moment, which is often the difference between an explainable event and a worrying one. These notes are only saved when they land on the same message as an actual warning, so this diary never fills a recording by itself.',
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
