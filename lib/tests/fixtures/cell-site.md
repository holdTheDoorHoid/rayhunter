# Cell Site regression packets

These are two individual HDLC frames extracted from a user-supplied QMDL,
not recordings. Hex includes the original DIAG header, CRC and HDLC terminator.
The original recording is kept outside the repository and must not be committed.

- `cell-site-timing-v1.hex`: `0xB114` LL1 Serving Cell Timing, version 1,
  20 adjustment records, including negative adjustments. It carries timing,
  not PCI, EARFCN, RSRP, RSRQ or RSSI.
- `cell-site-rrc-v12.hex`: `0xB0C0` LTE RRC, version 12, PDU 15 (UL-DCCH),
  PCI 219, EARFCN 3350. The two-byte RRC payload is a connection
  reconfiguration completion; it contains no subscriber identity.

Independent HDLC/CRC inspection of the complete capture found 30,118 valid
frames: zero `0xB17F`, zero `0xB180`, 24,606 `0xB114` (all version 1),
371 `0xB0C0` (all version 12), 2,972 `0xB063`, 2,165 `0xB064`, and four NAS
logs. All frames also parsed successfully with Rayhunter's existing parser.
There were no unsupported ML1 versions or LL1 parsing failures to fix.

The RRC traffic includes PCCH, DL-DCCH and UL-DCCH, but no SIB1. It can name
the connected cell by PCI/EARFCN; it cannot supply its global identity or
signal measurements. Paging alone must not select a serving cell.
