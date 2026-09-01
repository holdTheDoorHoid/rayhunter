# Summary

[Introduction](./introduction.md)

# Start here

- [What This Does, In Plain Terms](./what-this-does.md)
- [Is This For Me?](./threat-models.md)
- [Quick Start: From Box to First Recording](./quick-start.md)
- [Your First Warning](./first-warning.md)

# Understanding the problem

- [How Cell Networks Work](./concepts/cell-networks.md)
- [What a Cell-Site Simulator Is](./concepts/cell-site-simulators.md)
- [How These Attacks Actually Work](./concepts/attacks.md)
  - [Capturing Your Identity](./concepts/attack-identity.md)
  - [Downgrading You to Weaker Networks](./concepts/attack-downgrade.md)
  - [Turning Off Encryption](./concepts/attack-encryption.md)
  - [Making Your Phone Report Its Position](./concepts/attack-location.md)
- [Why Detection Is Hard](./concepts/why-detection-is-hard.md)
- [Reading Warnings Without Panicking](./concepts/interpreting-warnings.md)
- [What This Tool Cannot Tell You](./concepts/limitations.md)
- [Legal and Personal Risk](./concepts/risk.md)

# Installing

- [Choosing a Device](./supported-devices.md)
- [Installation](./installation.md)
  - [Installing from a Release](./installing-from-release.md)
  - [Installing from Source](./installing-from-source.md)
  - [Updating](./updating-rayhunter.md)
  - [Uninstalling](./uninstalling.md)
- [Device Notes](./devices/index.md)
  - [Orbic/Kajeet RC400L](./orbic.md)
  - [TP-Link M7350](./tplink-m7350.md)
  - [TP-Link M7310](./tplink-m7310.md)
  - [TP-Link M7200](./tplink-m7200.md)
  - [T-Mobile TMOHS1](./tmobile-tmohs1.md)
  - [UZ801](./uz801.md)
  - [Wingtech CT2MHS01](./wingtech-ct2mhs01.md)
  - [PinePhone and PinePhone Pro](./pinephone.md)
  - [Moxee Hotspot](./moxee.md)
  - [Porting to a New Device](./porting.md)

# Using it

- [Everyday Use](./using-rayhunter.md)
- [The Web Interface, Panel by Panel](./web-interface.md)
- [The Device Screen](./device-display.md)
- [Configuration](./configuration.md)
- [Recordings: Naming, Notes, and Rotation](./recordings.md)
- [Re-analyzing Recordings](./reanalyzing.md)
- [Analyzing a Capture Yourself](./analyzing-a-capture.md)
- [The Packet Explorer](./packet-explorer.md)
- [Securing the Web Interface](./web-authentication.md)
- [Sharing What You Find](./sharing-findings.md)

# The detectors

- [How Detection Works](./heuristics.md)
- [Severity, and What It Means](./severity.md)
- [Detector Reference](./detectors/index.md)
  - [Identity Requested Without Authentication](./detectors/imsi-requested.md)
  - [Redirected to 2G](./detectors/connection-redirect-downgrade.md)
  - [2G/3G Advertised Above 4G](./detectors/priority-2g-downgrade.md)
  - [Encryption Disabled (RRC)](./detectors/null-cipher.md)
  - [Encryption Disabled (NAS)](./detectors/nas-null-cipher.md)
  - [Incomplete System Information](./detectors/incomplete-sib.md)
  - [Location Requested (LPP)](./detectors/lpp.md)
  - [Location Requested on 2G (RRLP)](./detectors/rrlp.md)
  - [A Tower That Seems to Have Moved](./detectors/timing-advance.md)
- [Writing a New Detector](./detectors/writing-a-detector.md)
- [How We Validate Detectors](./detectors/validation.md)

# This fork

- [Why This Fork Exists](./fork/index.md)
- [What It Adds](./fork/features.md)
- [Differences You Will Notice](./fork/differences.md)
- [Compatibility With Upstream](./fork/compatibility.md)
- [Contributing Upstream](./fork/upstreaming.md)

# Reference

- [Configuration Reference](./configuration-reference.md)
- [REST API](./api-docs.md)
- [Report Format](./report-format.md)
- [Glossary](./glossary.md)
- [Sources and Further Reading](./references.md)
- [Frequently Asked Questions](./faq.md)
- [Troubleshooting](./troubleshooting.md)
- [Support, Feedback, and Community](./support-feedback-community.md)

---

[Legal Disclaimer](./disclaimer.md)
