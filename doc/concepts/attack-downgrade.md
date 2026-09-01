# Downgrading You to Weaker Networks

Your phone shows 4G. A few seconds later, without you touching it, it shows
2G, the mobile network technology designed in the late 1980s. You did not
walk into a coverage dead zone. You were told to move, by a tower, and your
phone did as it was told because that is what phones are built to do.

Being pushed onto an old network is rarely the whole attack. It is the setup.
The old networks lack the defenses the modern ones added, so moving you there
first is how an operator clears the way for the two techniques that follow, [turning off encryption](./attack-encryption.md) and, on 2G,
[asking your phone for its location](./attack-location.md). This page explains
why the old networks are weaker and the two ways your phone is moved onto
them. It maps to two of Rayhunter's detectors:
[Redirected to 2G](../detectors/connection-redirect-downgrade.md) and
[2G/3G Advertised Above 4G](../detectors/priority-2g-downgrade.md).

## What you would see

If you happen to be looking, the network label on your phone changing from 4G
or 5G to 2G or 3G, and staying there. On its own that is not alarming, phones
drop to older networks legitimately all the time, wherever 4G coverage is thin.
There is no way, from the label alone, to tell an ordinary fallback from a
deliberate push. That ambiguity is the whole difficulty of this detection, and
the reason the geographic caveat below matters so much.

## Why it matters

Recall from [How Cell Networks Work](./cell-networks.md) that each generation
of network fixed security weaknesses in the one before it. Two of those fixes
are what an attacker wants to undo:

- **On 2G, the tower never proves who it is.** The proof step of the joining
  ceremony runs one way only: your phone proves itself to the network, and the
  network proves nothing back. A 2G phone cannot tell a real tower from an
  impostor. On 4G, the network must prove itself or your phone walks away, so
  moving you to 2G removes the very check that would expose a fake tower.
- **2G's encryption is broken.** The scrambling used on 2G has known,
  practical breaks, old enough and well enough understood that traffic on it
  can be read outright ([Sources and Further Reading](../references.md), EFF's
  white paper).

Put those together and 2G is a network where your phone cannot detect a
fraud and cannot keep a secret. That is precisely the ground an operator wants
you standing on before they try to read your traffic. It would be a mistake,
though, to think only 2G is at risk: researchers have demonstrated practical
attacks against 4G/LTE itself on real networks (Shaik et al., 2016,
[Sources and Further Reading](../references.md)). The downgrade is not the only
way in. It is the cheapest and most reliable one.

## How it works

There are two ways to move a phone, and they match the two ways a phone
changes towers at all, from [How Cell Networks Work](./cell-networks.md):
being told to move, and deciding to move.

### Being told: redirection

When your phone is actively connected, the network can end that connection and
name where the phone should go next. Used honestly, this balances load and
manages coverage. Used as an attack, the named destination is a 2G cell. Your
phone releases its 4G connection and reconnects on 2G because it was
instructed to, and instructions like this carry no proof they are genuine.

This is the more direct and more suspicious of the two, because it is aimed at
your phone specifically and its only purpose here is to move you down.
Rayhunter's [Redirected to 2G](../detectors/connection-redirect-downgrade.md)
detector watches for exactly this: a connection-release message whose named
destination is a 2G network.

### Deciding: poisoned priorities

An idle phone chooses for itself which tower to prefer, using the priority
lists every tower broadcasts about its neighbours, again from
[How Cell Networks Work](./cell-networks.md). Normally a well-run tower ranks
its modern 4G neighbours highest, so idle phones stay on the good networks.

Because those broadcasts are unencrypted and unsigned, an attacker can transmit
their own, advertising 2G or 3G neighbours as *higher* priority than the 4G
ones. Idle phones nearby, following the rules faithfully, drift down onto the
old network on their own, no message aimed at any particular phone, nothing
that looks like an order. It is a quieter method that steers many phones at
once. Rayhunter's
[2G/3G Advertised Above 4G](../detectors/priority-2g-downgrade.md) detector
looks for these broadcasts that rank old networks above nearby 4G.

## The geographic caveat, which is the whole difficulty

Here is where reading a downgrade warning takes real care. **In much of the
world, 2G is not an antique, it is the working network.** In the United
States every major carrier except T-Mobile has shut down 2G and 3G, so on US
soil a drop to 2G is genuinely unusual. In many other countries, 2G and 3G
carry ordinary traffic every day, and a phone using them is doing nothing
remarkable ([Sources and Further Reading](../references.md), EFF's 2023 post
on platform 2G-disable settings).

So the same warning means different things in different places. What stays
constant, and what these detectors are really looking for, is not *2G exists*
but *a tower moved me there*, a redirection aimed at your phone, or a
priority list that inverts the normal ranking. Even so, in a region where 2G
is in daily use, the benign explanations for such a warning are far more
numerous, and it should be weighed accordingly.
[Reading Warnings Without Panicking](./interpreting-warnings.md) gives the
method for exactly that kind of judgement, and it is worth reading before you
act on a downgrade warning anywhere.

## The precise details

- **Redirection** is an LTE RRC *Connection Release* carrying a
  `redirectedCarrierInfo` that names a GERAN (2G) target; the detector at
  `lib/src/analysis/connection_redirect_downgrade.rs` raises a high-severity
  warning on that specific case. Its origin is a HITBSecConf presentation on
  forcing LTE phones onto eavesdropping networks (cited in the code).
- **Priority poisoning** lives in the *System Information Blocks* that carry
  reselection priorities, SIB6 for 3G neighbours, SIB7 for 2G, weighed
  against the LTE priorities in SIB3 and SIB5. The detector at
  `lib/src/analysis/priority_2g_downgrade.rs` warns when a legacy priority
  outranks the LTE one, and derives from heuristic T7 in Shinjo Park's "Why We
  Cannot Win." Its earlier versions raised many false alarms; the current
  version is stricter, a history the detector page
  [2G/3G Advertised Above 4G](../detectors/priority-2g-downgrade.md) tells in
  full.
- **That 4G is attackable too**, not only 2G, is Shaik et al., NDSS 2016
  ([Sources and Further Reading](../references.md)), the first publicly
  reported practical attacks against LTE, on commercial phones and real
  networks.
- Both detectors watch what a tower broadcasts or sends to this device; a
  downgrade accomplished by other means, or aimed elsewhere, may leave nothing
  for them, [What This Tool Cannot Tell You](./limitations.md).

## Where to next

[Turning Off Encryption](./attack-encryption.md) is what a downgrade most
often clears the way for.
