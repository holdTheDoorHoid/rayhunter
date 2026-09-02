# Heuristics

Rayhunter includes several analyzers to detect potential IMSI catcher activity. These can be enabled and disabled in your [configuration](./configuration.md) file.

## Available Analyzers

### IMSI Requested (v3)

This analyzer tests whether the eNodeB sends an IMSI or IMEI Identity Request NAS message under suspicious .

Mobile networks primarily request IMSI or IMEI from a mobile device during initial network attachment or when the network cannot identify the mobile device by its temporary identification (TMSI - *Temporary Mobile Subscriber Identity* or GUTI - *Globally Unique Temporary Identifier* in 4G/5G terminology).

IMSI request therefore usually happens when you first turn the device on especially after it has been off for a long time. Another possibility is, that you reboot your mobile device and your temporary ID expired. Sometimes temporary identification can expire if you have been in an area where there is absolutely no connection to your service provider or after you left your device on an airplane mode and then reconnect to the network (especially being disconnected for a long time). IMSI could also be requested when you connect to a new network (for instance for roaming), when you swap she SIM card or when your device moves to a new *Tracking Area* or *Location Area* and the network can not map the temporary identification to your device. IMSI number can also be requested after core network reboot.

It should also be noted that the network periodically reassigns your device new temporary identification to enhance security and avoid tracking, but in that cases usually does not request IMSI.

During these events the phone will typically go on to authenticate that the network is legitimate and then establish service with the network it is connected to. 

What we consider suspicious is the following chain of events:

* Phone connects to a new tower. 
* Tower asks for phones identity (IMEI or IMSI.)
* Authentication does *NOT* happen. 
* Tower requests phone to disconnect. 

Looking for this chain of events is much less prone to false positives than naively looking for any time the IMSI/IMEI is sent. We do still sometimes get false positives when users are in an airplane that is coming in for a landing however. This is likely due to having been disconnected for a while and then being over towers that are not able to route to your home network, but we are still researching.

This is the attack used by commercial IMSI catchers used by law enforcement. 

This heuristic will also alert you if any of the following happen:
* Identity is requested after authentication.
* Identity is requested without your phone connecting to the tower. 
* Identity is requested and then authentication doesn't happen shortly thereafter. 

This heuristic will also issue a notification every time your identity is sent to the network under non suspicious circumstances. This is for diagnostic purposes. 

### Connection Release/Redirected Carrier 2G Downgrade

This analyzer tests if a base station releases your device's connection and redirects your device to a 2G base station. This heuristic is useful, because some IMSI catchers may operate in a such way that they downgrade connection to 2G where they can intercept the communication (by performing man-in-the-middle attack).


### LTE SIB6/7 Downgrade (v2)

This analyzer tests if LTE base station is broadcasting a SIB type 6 and 7 messages which include 2G/3G frequencies with higher priorities.

SIB (*System Information Block*) Type 6 and 7 are specific types of broadcast messages sent by the base station (eNodeB in 4G networks) to mobile devices. They contain essential radio-related configuration parameters to help mobile device perform cell reselection.

This attack exploits the fact that SIB broadcast messages are not encrypted or authenticated. This allows them to pretend to be a legitimate cell by broadcasting fake system information in order to force mobile devices to downgrade from more secure 4G (LTE) to less secure 2G (GSM) network and then steal IMSI and/or perform man-in-the-middle attack. That is why this is also called a downgrade attack.

SIB6 is used for cell reselection to CDMA2000 systems which are not supported by many modern mobile phones, and SIB7 Provides the mobile device with information to perform cell reselection to GSM/EDGE networks. Therefore SIB6 messages are quite rare, while malformed SIB7 messages are much more frequent in practice. 

This heuristic is useful even in countries where 2g is still prevalent. A well behaved tower should always advertise its other 4g neighbors at a higher priority than 2g/3g neighbors. (Older versions of this heuristic were prone to false positives.)

### Null Cipher

This analyzer tests whether the cell suggests using a null cipher (EEA0) in the RRC layer. That means that encryption between your mobile device and base station is turned off.

Normally this should never happen, because null cipher is used almost exclusively for testing and debugging in labs or in controlled environments. Sometimes null cipher is used if encryption negotiation fails or isn’t supported (however in most networks this should not be the case). Also, some regulations allow unencrypted communications in **specific** emergency cases.

The general rule is that null cipher should never be used in commercial deployments, except in very controlled conditions (e.g., test labs) or in a very specific regulatory-approved use cases.

On the other hand, IMSI catchers often use null cipher to avoid setting up secure contexts (because they lack valid keys) and/or to trick mobile device into using unencrypted links (which makes eavesdropping easier).

### NAS Null Cipher

This analyzer tests whether the security mode command at the NAS layer suggests using a null cipher (EEA0). This would usually only happen after a mobile device has successfully authenticated with the MME (*Mobility Management Entity* - core network component that handles signaling and control) but still it shouldn't happen at all. This could be indicative of an attack though using SS7 (*Signaling System 7* - a set of telecommunication protocols used to set up and manage calls and other services) to get key material from the HLR (*Home Location Register* - a database in mobile telecommunications networks that stores subscriber information) of the mobile phone for a successful authentication.

It could also indicate an IMSI catcher which is connected to the mobile network MME and HLR through cooperation between government and telecom provider. Or it could be a false positive if the telecom provider is intending to use null ciphers (if encryption is illegal in some country, or they have some misconfiguration of the network), however this should be very rare case.

### Incomplete SIB

This analyzer tests whether the SIB1 message contains a complete SIB chain (SIB3, SIB5, etc.). A legitimate SIB1 message should contain timing information for at least 2 additional SIBs (SIB3, 4, and 5 being the most common) but a fake base station will often not bother to send additional SIBs beyond 1 and 2 (i. e. some IMSI catchers send just SIB1 and *one additional* SIB).

On its own this might just be a misconfigured base station (though we have only seen it in the wild under suspicious circumstances) but combined with other heuristics such as **IMSI Requested** detection it should be considered as a strong indicator of malicious activity.

### LPP Location Request

This analyzer watches for LPP (*LTE Positioning Protocol*, 3GPP TS 36.355) messages, the mechanism a network uses to ask a device to measure and report its own position: GNSS/GPS readings, timing measurements of nearby cells (ECID/OTDOA), or a computed location estimate. LPP messages travel inside NAS Generic NAS Transport messages (3GPP TS 24.301, container type 1), which is where this analyzer reads them.

LPP exists for emergency services and lawful location services, but the same machinery allows whoever controls the network connection, a cooperating carrier, or an IMSI catcher acting as the network, to ask a device for its precise position at any time, continuously, with nothing visible on the device. Its use in real tracking operations is what motivated this heuristic (see EFForg/rayhunter#1072).

A *request for location information* arriving from the network, and the device's *report of location information* back, each raise a **low** severity warning, once per LPP transaction: periodic reporting sessions can send a report every few seconds for hours, and repeating the warning for every report would bury the rest of the history. Repeats within the same transaction, capability exchanges, assistance data (routine GPS ephemeris help), aborts, and errors are recorded as informational events.

False positives: an emergency call will legitimately trigger LPP, and some carriers use network-initiated location for lawful services (fraud checks, "find my device" offerings, regulatory obligations). A warning during a normal day with no emergency call, especially on a stationary hotspot, is worth reading alongside the other heuristics.

### LPP Location Tracking

This is the in-depth companion to **LPP Location Request**. Where that analyzer reports *that* a location message was exchanged, this one decodes the request and response bodies to report *what kind*: which positioning method the network asked for (A-GNSS satellite, OTDOA tower-timing, or E-CID cell-ID), and, the point of the analyzer, whether the request is for a single fix or for **periodic (continuous) reporting**.

A `periodicalReporting` field in a `RequestLocationInformation` means the network asked the device to report its position repeatedly on a timer until told to stop. That is the signature of active tracking rather than a one-off locate, so it is raised to **medium** severity, versus **low** for a one-shot request or a position report. Capability and assistance messages carry no such detail and are left to the basic analyzer.

It is kept as a separate, independently toggleable analyzer because it parses more of each LPP message than the basic check. A device very short on memory can disable `lpp_location_tracking` and keep `lpp_location_request`; enabling both is fine and gives the fullest picture. The extra bytes read all sit at fixed offsets in the message with no variable-length content in front of them, which is what makes decoding them by hand safe; the offsets are verified against a reference 36.355 encoder in the tests.

Same false positives as **LPP Location Request**: emergencies and lawful location services. A *continuous* request with no emergency in progress is the one most worth attention.

### RRLP Location Request

The 2G counterpart to the LPP analyzers, addressing EFForg/rayhunter#534. RRLP (Radio Resource LCS Protocol, 3GPP TS 44.031) is how the older GSM network asks a handset to measure and report its position, from before LPP existed.

On the air, an RRLP message travels inside a GSM RR **APPLICATION INFORMATION** message (3GPP TS 44.018 §9.1.53). This analyzer reads that transport header to find the RRLP APDU, then reads the front of the APDU to tell a location *request* (`msrPositionReq`) from the device's *response* (`msrPositionRsp`) or from routine assistance data. A request or a response warns at **low** severity; assistance data, acknowledgements and errors are informational.

This matters because being forced down to 2G is itself a known surveillance step (see **Pushed down to a 2G network**), and RRLP is how a device is then located there. The transport framing is verified in the tests against `pycrate_mobile`'s 44.018 implementation, and the RRLP APDU against pycrate's 44.031 ASN.1. Detection requires both a valid APPLICATION INFORMATION header *and* a well-formed RRLP APDU behind it, so an unrelated 2G message cannot raise a false positive.

False positives: legitimate on emergency calls placed over 2G. On a device that never uses 2G, it stays silent.

### FlashCatch: identity taken, then forged authentication (v1)

A conventional IMSI catcher holds the phone after taking its identity, and the phone loses service until it gives up. The FlashCatch attack (Paci, Bologna, Palamà and Bianchi, ACM WiSec 2025) avoids that: posing as a tower of the phone's own network, it asks for the IMSI the moment the phone checks in, then sends three authentication challenges it has signed wrongly. The phone rejects each as forged (**AUTHENTICATION FAILURE**, cause "MAC failure", TS 24.301 §9.9.3.9), treats the tower as having failed authentication after the third (§5.4.2.6), bars the cell and returns to the real network with its keys intact, all in under a second and with no visible interruption.

This analyzer counts those rejections. Two in a row within one exchange raise a **medium** warning; when an **IDENTITY REQUEST** for the IMSI arrived shortly before, the warning is **high**, because that is the whole attack. Three "Synch failure" rejections in a row also raise a medium warning. An identity request during a tracking area update is noted at informational level only. A passed authentication, security mode command, or any accept message clears the count, and a new attach, tracking area update or service request starts afresh.

False positives: a SIM whose key does not match the network (a badly provisioned or test SIM) fails authentication on every connection and would trip this constantly, but such a phone has no service, so the situation is obvious. A single rejection is not counted. The pattern comes from the paper's description; it has not yet been checked against a recording of the attack. See the [detector page](./detectors/flash-catch.md).

### Diagnostic Information 
This analyzer displays some diagnostic information about when your device connects and disconnects from certain towers. It is helpful for analysis of suspicious PCAPs. The informational warnings in here can safely be ignored until there is a low, medium, or high severity warning. 

### Test Analyzer

This analyzer is great for testing if your Rayhunter installation works. It will alert every time a new tower is seen (specifically every time a tower broadcasts a SIB1 message.) It is designed to be very noisy so we do not recommend leaving it on but if this alerts it means your Rayhunter device is working! 
