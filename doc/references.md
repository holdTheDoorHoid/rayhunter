# Sources and Further Reading

Everything in these docs that makes a claim about how cellular networks behave, how
cell-site simulators work, or how often they are used should be traceable to something on
this page. If you find a claim in these docs that is not, that is a bug, please report it.

Entries are annotated so you can tell, before you click, whether a source is worth your
time and what it will assume you already know. Difficulty is marked as:

- **Accessible**, written for a general audience, no background needed
- **Technical**, assumes some networking background, but self-contained
- **Research**, academic paper, assumes familiarity with the field

---

## Start here

### EFF, "Cell-Site Simulators / IMSI Catchers" (Street-Level Surveillance)
<https://sls.eff.org/technologies/cell-site-simulators-imsi-catchers>
**Accessible.** The best short introduction. Covers what these devices are, how police use
them, the legal landscape, and the state of detection. EFF's Street-Level Surveillance
project is aimed at advocacy organizations, journalists, and defense attorneys, so it is
written to be usable by people who are not engineers. Start here if you are new.

### EFF, "Meet Rayhunter: A New Open Source Tool from EFF to Detect Cellular Spying" (2025)
<https://www.eff.org/deeplinks/2025/03/meet-rayhunter-new-open-source-tool-eff-detect-cellular-spying>
**Accessible.** The announcement post for the project this fork descends from. Explains the
motivation: very little is publicly known about how commercial cell-site simulators work or
where they are deployed, and previous detection approaches required either a rooted Android
phone or expensive software-defined radio equipment.

### EFF, "A Quick and Dirty Guide to Cell Phone Surveillance at Protests" (2020)
<https://www.eff.org/deeplinks/2020/06/quick-and-dirty-guide-cell-phone-surveillance-protests>
**Accessible.** Practical threat modeling for the most common high-risk scenario. Also
surveys the detection projects that came before: SeaGlass and SITCH for 2G, EFF's own
Crocodile Hunter for 4G.

---

## The core technical reference

### Nasser, Y. "Gotta Catch 'Em All: Understanding How IMSI-Catchers Exploit Cell Networks." EFF, 2019
<https://www.eff.org/wp/gotta-catch-em-all-understanding-how-imsi-catchers-exploit-cell-networks>
PDF: <https://www.eff.org/files/2019/07/09/whitepaper_imsicatchers_eff_0.pdf>
**Technical**, but deliberately self-contained. This is the single most important source for
these docs. EFF wrote it specifically to bridge the gap between low-level academic research
and high-level posts that never explain the mechanics. It builds up from the necessary
background, then walks through the attack categories: the basic IMSI-catcher, communication
interception, authentication spoofing, and how encryption is dealt with.

Directly relevant to several of our detectors: it explains that paging messages are usually
addressed to a temporary identifier but sometimes to the permanent one, that unencrypted
paging channels can be monitored by anyone, and that phones periodically transmit
unencrypted measurement reports which can be enough to compute position, sometimes
including exact GPS coordinates.

Released under a Creative Commons Attribution license, so you may quote it freely with
credit.

---

## Academic research

### Dabrowski, A., Pianta, N., Klepp, T., Mulazzani, M., Weippl, E. "IMSI-Catch Me If You Can: IMSI-Catcher-Catchers." ACSAC 2014
DOI: <https://doi.org/10.1145/2664243.2664272>
Open preprint: <https://publications.sba-research.org/publications/AdrianDabrowski-IMSI-Catcher-Catcher-ACSAC2014-preprint-20140820.pdf>
**Research.** The foundational detection paper, this is where the detection-by-artifact
approach our heuristics use comes from. It identifies and describes multiple methods of
detecting the artifacts a catcher leaves in the mobile network.

Its taxonomy of operating modes is worth internalizing, because our detectors map onto it:

- **Identification mode**, the phone is lured in, its permanent identifiers are read, and
  it is then pushed back to its real network by rejecting its location update. This is the
  exact pattern our identity detector looks for.
- **Camping mode**, the phone is held on the fake cell and its traffic collected, forwarded
  on to the real network so the user notices nothing.

The paper also notes the economics: once vendor count rose and prices fell, self-built
devices based on open source software became available for roughly US$1,500, which is what
moved these from a government-only capability to a widely available one.

### Ney, P., Smith, I., Cadamuro, G., Kohno, T. "SeaGlass: Enabling City-Wide IMSI-Catcher Detection." PoPETs 2017(3), 39–56
DOI: <https://doi.org/10.1515/popets-2017-0027>
<https://petsymposium.org/popets/2017/popets-2017-0027.php>
**Research.** University of Washington. Sensors in 15 ridesharing vehicles across two
cities, two months of data each. Important to us for two reasons. First, it establishes the
method of learning a city's normal network behavior and then flagging deviations from it, the same logic behind treating an unfamiliar cell as interesting. Second, its framing of the
problem: the richest public information about US government use comes from anonymous leaks,
public records requests, and court proceedings, which is precisely the deficiency
distributed detection is meant to address.

### Shaik, A., Borgaonkar, R., Asokan, N., Niemi, V., Seifert, J-P. "Practical Attacks Against Privacy and Availability in 4G/LTE Mobile Communication Systems." NDSS 2016
DOI: <https://doi.org/10.14722/ndss.2016.23236>
Preprint: <https://arxiv.org/pdf/1510.07563>
**Research.** The first publicly reported practical attacks against LTE access network
protocols, demonstrated with commercial phones on real networks. Cite this whenever the docs
claim that 4G is attackable rather than merely that 2G is. Its location-leak results
quantify the exposure: a semi-passive attacker can place a device within about a 2 km² area
in a city, and an active attacker can obtain precise position via GPS coordinates or
trilateration from signal-strength measurement reports. That last mechanism is what our LPP
and RRLP detectors watch for.

### Park, S., Shaik, A., Borgaonkar, R., Martin, A., Seifert, J-P. "White-Stingray: Evaluating IMSI Catchers Detection Applications." USENIX WOOT 2017
**Research.** Builds a catcher, then tests the detector apps against it. Essential reading
before writing anything confident about detection reliability: it demonstrates that apps
implementing nominally the same detection methods do not necessarily agree on whether a
given base station is suspicious. Use this to support the honesty in
[What This Tool Cannot Tell You](./concepts/limitations.md).

---

## Countermeasures and the 5G picture

### EFF, "Apple and Google Are Introducing New Ways to Defeat Cell Site Simulators, But Is it Enough?" (2023)
<https://www.eff.org/deeplinks/2023/09/apple-and-google-are-introducing-new-ways-defeat-cell-site-simulators-it-enough>
**Accessible.** Covers platform-level 2G-disable settings. Also gives the geographic caveat
that matters for our downgrade detectors: in the US every major carrier except T-Mobile has
shut down 2G and 3G, but many countries outside the US have not, which changes how you
should read a 2G-related warning depending on where you are.

### EFF, "The 5G Protocol May Still Be Vulnerable to IMSI Catchers" (2019)
<https://www.eff.org/deeplinks/2019/01/5g-protocol-may-still-be-vulnerable-imsi-catchers>
**Accessible.** Why the identity-encryption improvements in 5G do not close the problem.

### EFF, "Crocodile Hunter"
<https://www.eff.org/pages/crocodile-hunter> · <https://github.com/EFForg/crocodilehunter>
**Technical.** The predecessor project: a software-defined radio tool that listens for
broadcast messages from 4G base stations, infers their location, and looks for unusual
activity. Useful context for why a passive, cheap, hotspot-based approach was worth building
instead.

---

## Protocol specifications

Free to download from <https://www.3gpp.org/ftp/Specs/archive/>. These are the authority
when a detector's behavior is questioned. Cite the clause, not just the document.

| Spec | Title | Where it matters here |
|---|---|---|
| TS 36.331 | E-UTRA Radio Resource Control (RRC) | System information blocks, redirection, cell reselection priorities |
| TS 24.301 | NAS protocol for EPS | Identity request/response, security mode command, attach reject |
| TS 33.401 | 3GPP System Architecture Evolution: Security architecture | Which cipher and integrity algorithms are permitted, including the null algorithms |
| TS 36.355 | LTE Positioning Protocol (LPP) | The LPP detectors, request/response structure, positioning methods, periodic reporting |
| TS 44.031 | Radio Resource LCS Protocol (RRLP) | The 2G location detector |
| TS 44.018 | GSM/EDGE RRC protocol | The transport framing RRLP arrives in |
| TS 36.321 | E-UTRA Medium Access Control (MAC) | The random access response and its timing advance command, read by the timing-advance detector |

---

## Human factors and documentation craft

Not about cellular networks, about writing docs people can actually act on. These inform
[the writing standards](../STYLE.md) rather than the technical content.

- Procida, D. **Diátaxis.** <https://diataxis.fr/>, the four documentation modes and why
  mixing them makes both halves worse.
- Sweller, J., Ayres, P., Kalyuga, S. **Cognitive Load Theory.** Springer, 2011, why
  dumping everything on one page teaches less than staging it.
- Mayer, R. E. **Multimedia Learning.** 3rd ed., Cambridge, 2020, signaling, segmenting,
  and pre-training principles, applied here to diagrams and to teaching vocabulary before
  process.
- Carroll, J. M. **The Nurnberg Funnel: Designing Minimalist Instruction for Practical
  Computer Skill.** MIT Press, 1990, action-oriented documentation and designing for error
  recovery.
- Lee, J. D., See, K. A. "Trust in Automation: Designing for Appropriate Reliance."
  *Human Factors* 46(1), 2004, 50–80, the framework behind
  [Reading Warnings Without Panicking](./concepts/interpreting-warnings.md). Appropriate
  reliance, not maximum trust, is the goal.
- Parasuraman, R., Riley, V. "Humans and Automation: Use, Misuse, Disuse, Abuse."
  *Human Factors* 39(2), 1997, 230–253, why users abandon detectors that cry wolf, and why
  false-positive honesty in our detector pages is a safety feature rather than a weakness.
