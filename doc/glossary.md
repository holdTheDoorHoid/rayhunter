# Glossary

Every specialised term this book uses, defined in one or two plain sentences.
Where a term has a plain phrase and a piece of jargon, the plain phrase comes
first and the jargon follows in parentheses — the way the rest of the book
introduces them.

Terms are alphabetical by their plain name. Acronyms are cross-referenced to the
plain term.

### A-GNSS (assisted satellite positioning)

A way for the network to help your phone get a satellite (GPS) location fix
faster, by supplying data about which satellites to look for. The most precise
of the [positioning](#positioning) methods, and one the [LPP](#lpp) location
detectors watch for.

### attach

The short joining ceremony your phone and the network run through when your
phone connects: it asks to join, may be asked who it is, the network proves
itself ([authentication](#authentication)), encryption switches on, and the
phone is accepted. Many attacks operate in the early part of this ceremony,
before the proof step.

### authentication

The step where the network proves it is really your carrier, using secrets
stored on your SIM card, and your phone checks the proof. On 4G the network
*must* pass this or your phone walks away; on [2G](#generations-2g-3g-4g-5g) the
network is not required to prove itself at all.

### base station

See [tower](#tower).

### broadcast messages

The announcements every [tower](#tower) transmits continuously about itself —
which carrier it belongs to, its neighbours, and how to connect. They are
unencrypted and carry no signature, so anyone can listen to them and anyone with
the right equipment can transmit imitations. Carried in numbered blocks called
[System Information Blocks](#system-information-block-sib).

### camping (mode)

One of the two ways a [cell-site simulator](#cell-site-simulator) is used: it
keeps your phone connected and sits between you and the real network, relaying
your traffic while watching it. Contrast [identification](#identification-mode).

### cell

One patch of network coverage, served by one [tower](#tower). The origin of the
term "cell phone."

### cell-site simulator

A device that pretends to be a real [tower](#tower) so that phones nearby connect
to it and reveal information about themselves. Also called an **IMSI catcher** or
a **Stingray** (originally one manufacturer's product name). This book treats the
three names as the same thing.

### downgrade

Being moved from a modern network onto an older, weaker one — usually
[2G](#generations-2g-3g-4g-5g) — where protections are absent or breakable.
Often the setup for [turning off encryption](#null-cipher) or reading traffic.

### E-CID (Enhanced Cell ID)

A [positioning](#positioning) method based on which [cell](#cell) your phone is
in plus timing to it. The coarsest of the methods, but cheap and quiet.

### EEA0 (the null cipher)

The standardised "no encryption at all" option in LTE. It exists for narrow
legitimate reasons (such as unauthenticated emergency calls) but is the tell of a
[cell-site simulator](#cell-site-simulator) that holds no keys. See [null
cipher](#null-cipher).

### GERAN

The technical name for the [2G](#generations-2g-3g-4g-5g) radio network. When a
detector reports a redirect to GERAN, it means a redirect to 2G.

### generations (2G, 3G, 4G, 5G)

Successive designs of mobile network, each fixing security weaknesses in the
last. 2G (early 1990s) does not require the network to prove itself and has
breakable encryption; 4G (LTE) requires mutual proof and stronger encryption.
Phones keep the old generations for coverage and fall back to them, which is what
a [downgrade](#downgrade) exploits.

### GUTI

The larger temporary identity LTE assigns, which wraps the
[TMSI](#tmsi-temporary-name). This book says "temporary name" or TMSI for the
whole idea of the rotating alias.

### handover

Being moved from one [tower](#tower) to another by the network while your phone
is active (for example, mid-call), based on signal measurements your phone
reports. Contrast [reselection](#reselection), which an idle phone does for
itself.

### identification (mode)

One of the two ways a [cell-site simulator](#cell-site-simulator) is used: it
lures your phone in, reads its permanent identity, and pushes it back to the real
network by rejecting the connection. The pattern the [identity
detector](./detectors/imsi-requested.md) watches for. Contrast
[camping](#camping-mode).

### IMEI (permanent hardware identity)

The permanent number identifying your phone's hardware. It survives a SIM swap,
which is part of why it interests trackers. Full name: International Mobile
Equipment Identity.

### IMSI (permanent SIM identity)

The permanent, globally unique number identifying your SIM card. It does not
change, so anyone who records it can recognise you again anywhere. This is the
number a [cell-site simulator](#cell-site-simulator) exists to collect — hence
"IMSI catcher." Full name: International Mobile Subscriber Identity.

### IMSI catcher

See [cell-site simulator](#cell-site-simulator).

### LPP (the modern positioning protocol)

The protocol a 4G network uses to ask your phone to measure and report its own
position. Legitimate for locating emergency calls; also the machinery a network
can use to [track you](./concepts/attack-location.md). Full name: LTE Positioning
Protocol (3GPP TS 36.355). Its 2G ancestor is [RRLP](#rrlp-the-2g-positioning-protocol).

### LTE

See [generations](#generations-2g-3g-4g-5g); LTE is 4G.

### NAS (the core-network conversation)

The stream of control messages between your phone and the carrier's core network
— attach, identity, authentication, and so on — as opposed to the messages
between your phone and the local tower ([RRC](#rrc-the-tower-conversation)).
Rayhunter reads both. Full name: Non-Access Stratum.

### null cipher

Encryption set to "none." A [tower](#tower) or the core network proposing it is
asking your phone to communicate unencrypted, readable by anyone listening. The
standardised option is called [EEA0](#eea0-the-null-cipher). See [Turning Off
Encryption](./concepts/attack-encryption.md).

### OTDOA (tower-timing positioning)

A [positioning](#positioning) method in which your phone measures the tiny
differences in when signals arrive from several [towers](#tower), pinning down
its location by geometry. Full name: Observed Time Difference Of Arrival.

### paging

The network calling out for an idle phone when a call or message arrives, on a
channel every phone in the area listens to. Usually uses the [temporary
name](#tmsi-temporary-name); sometimes the permanent [IMSI](#imsi-permanent-sim-identity),
and the channel is unencrypted.

### PLMN

The identifier of a mobile network operator (its country and network codes).
Rayhunter compares the PLMN a tower advertises with the one your phone expects,
to tell a home-network event from a roaming one.

### positioning

The general act of determining where a phone is. The network can ask the phone to
measure and report its own position using [A-GNSS](#a-gnss-assisted-satellite-positioning),
[OTDOA](#otdoa-tower-timing-positioning), or [E-CID](#e-cid-enhanced-cell-id),
over [LPP](#lpp-the-modern-positioning-protocol) or, on 2G,
[RRLP](#rrlp-the-2g-positioning-protocol).

### reselection

An idle phone re-deciding for itself which [tower](#tower) to prefer, using the
priorities carried in the [broadcasts](#broadcast-messages). Poisoning those
priorities is one form of [downgrade](#downgrade). Contrast
[handover](#handover).

### RRC (the tower conversation)

The stream of control messages between your phone and the local [tower](#tower) —
setting up the radio link, security, and handovers — as opposed to the
conversation with the core network ([NAS](#nas-the-core-network-conversation)).
Full name: Radio Resource Control.

### RRLP (the 2G positioning protocol)

The 2G ancestor of [LPP](#lpp-the-modern-positioning-protocol): the older
protocol a 2G network uses to ask a handset for its position. Watched for by the
[RRLP detector](./detectors/rrlp.md). Full name: Radio Resource LCS Protocol
(3GPP TS 44.031).

### Security Mode Command

The message in which the network tells your phone which encryption to switch on.
The place a [null cipher](#null-cipher) is proposed. There is one at the radio
layer ([RRC](#rrc-the-tower-conversation)) and one at the core-network layer
([NAS](#nas-the-core-network-conversation)).

### severity

Rayhunter's own grading of a finding: Informational, Low, Medium, or High. See
[Severity, and What It Means](./severity.md).

### SIB

See [System Information Block](#system-information-block-sib).

### Stingray

See [cell-site simulator](#cell-site-simulator).

### System Information Block (SIB)

One of the numbered [broadcast](#broadcast-messages) blocks a tower transmits,
each carrying a defined slice of its announcement — identity, neighbours,
reselection priorities. SIB6 and SIB7 carry 3G and 2G neighbour priorities, which
the [downgrade detector](./detectors/priority-2g-downgrade.md) reads.

### TMSI (temporary name)

The short-lived alias the network assigns your phone in place of the permanent
[IMSI](#imsi-permanent-sim-identity), changed from time to time so you cannot be
followed between sessions. LTE wraps it in a [GUTI](#guti). Full name: Temporary
Mobile Subscriber Identity.

### tower

The carrier equipment — radios and antennas — that serves one [cell](#cell) of
coverage and that your phone talks to directly. The technical term is *base
station* (an *eNodeB* on 4G). This book says "tower" throughout.

## Where to next

The vocabulary here is built up in context in [How Cell Networks
Work](./concepts/cell-networks.md). Every term's first use elsewhere in the book
links back here.
