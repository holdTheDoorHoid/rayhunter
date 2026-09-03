# rayhunter-collector

The community side of contributed recordings: an ingest service, triage
tools, and a static site generator, in one binary. Read `../DESIGN.md` for
what this is and why it is shaped this way. This file is for whoever runs
one.

## What you are taking on

Units send you bundles their owners chose to share. In the shareable tier
those hold captures with the device's own identifiers zeroed, analysis
reports, tower identities, and a rounded location. In the full tier they also
hold raw captures, encrypted to a key this server never has. You are the
person those owners are trusting. Review before you publish, publish only
what you would defend, and keep the archive key somewhere the server is not.

## Setup

```bash
cargo build --release -p rayhunter-collector
./target/release/rayhunter-collector keygen --out /srv/rayhunter/keys
```

`keygen` writes `ingest.key`, `ingest.pub`, `archive.key` and `archive.pub`,
and prints the fingerprints. **Move `archive.key` off the server now.** The
server refuses to start while it is in the key directory. Publish the two
fingerprints somewhere owners can compare them against what their device
shows.

Run the ingest service behind a TLS reverse proxy (Caddy, nginx) that
forwards to it:

```bash
./target/release/rayhunter-collector serve \
  --data /srv/rayhunter/data \
  --keys /srv/rayhunter/keys \
  --bind 127.0.0.1:8090 \
  --name "Example Community Rayhunter Dataset" \
  --contact "data@example.org" \
  --site-url "https://data.example.org" \
  --behind-proxy
```

Add `--accept-full` to take full submissions; it needs `archive.pub` in the
key directory. `--max-summary-mb` (64) and `--max-capture-mb` (512) cap each
part; `--max-disk-gb` (50) stops accepting when the data directory is larger.
`--behind-proxy` reads the client address from the last `X-Forwarded-For`
entry; never set it when clients can reach the port directly, or the rate
limiter can be lied to.

Units read `/.well-known/rayhunter-telemetry` from the same origin, so the
proxy must forward that path too.

## Triage

```bash
rayhunter-collector list --data /srv/rayhunter/data --status received
rayhunter-collector show --data /srv/rayhunter/data <id>
rayhunter-collector review --data /srv/rayhunter/data <id> \
  --status verified --tag interesting --tag vulnerable --note "IMSI request with no auth, twice, same cell" --reviewer you
```

`show` prints the record and the summary the unit sent, including the
events, the cells and the location. The shareable bundle itself is
`data/submissions/<id>/summary.zip`; open the `.pcapng` inside it in
Wireshark, or run `rayhunter-check` over it.

Status `verified` publishes; `rejected` keeps it out. Suggested tags:
`interesting`, `vulnerable`, `confirmed`, `false-positive`, `needs-capture`,
`baseline`.

To read a full submission's capture, on a machine that holds the archive key:

```bash
rayhunter-collector decrypt --data <copy of the data dir> <id> --archive-key archive.key --out capture.zip
```

The capture is never decrypted on the server and is never published.

## Publishing

```bash
rayhunter-collector publish --data /srv/rayhunter/data --out /srv/rayhunter/site \
  --title "Example Community Rayhunter Dataset" --base-url https://data.example.org
```

writes a static site: `index.html` (the list, filterable in the browser),
`map.html` (Leaflet, pinned versions with integrity hashes, OpenStreetMap
tiles), `s/<id>/index.html` per submission, `files/<id>/` with the bundle and
the files inside it, and `data/submissions.json`, `data/map.geojson`,
`data/submissions.csv`. Serve the directory from anything that serves files.
Run it from cron, or after each review.

Only `verified` submissions are published. A withdrawal removes the payload
at once and drops the entry from the next publish.

## Abuse

Submissions are signed; forged or altered ones are refused before anything
is written. Opens are limited to 30 per address per hour and 100 per
submitter key per day. To ban a submitter key, put its id in
`data/bans.json` as a JSON list of strings and restart. Keys rotate monthly
on the units, so a ban is a month-long inconvenience to an abuser; the real
defence is that nothing is published unreviewed.

## What is on disk

```text
data/
  bans.json                       optional, a list of banned key ids
  submissions/<id>/
    manifest.json                 exactly as received
    manifest.sig
    state.json                    status, review, counts
    parts/summary.enc             ciphertext, to the ingest key
    parts/capture.enc             ciphertext, to the archive key (full tier)
    summary.zip                   decrypted at finalize
    summary.json                  telemetry.json, extracted
```

No client addresses are written to disk. Unfinished submissions are
dropped after 24 hours.
