# Running a collection service

A runbook for one small server that receives contributions and serves the
public site. It assumes you have read `../collector/README.md` for what the
service is, and `../DESIGN.md` for why. Everything here is one machine, one
domain, and about an hour of work.

The files beside this README are the templates: `Caddyfile`,
`rayhunter-collector.service`, `rayhunter-collector-publish.service` and
`.timer`, and `about.html`. Replace `data.example.org`, the name and the
contact address in each.

## The shape

```text
owner's unit ──https──► Caddy (TLS, port 443) ──► rayhunter-collector (127.0.0.1:8090)
                             │                          data: /var/lib/rayhunter-collector
                             └── /var/www/rayhunter-data  (the published site, plain files)

reviewer's laptop: holds archive.key, never the server. Reviews over SSH.
```

Two secrets exist. `ingest.key` lives on the server so it can open summaries
as they arrive. `archive.key` is generated on your laptop and **never copied
to the server**; the server refuses to start if it finds one.

## 1. Domain

Register a domain (about $10 to $12 a year for a `.org`). Add an `A` record
(and `AAAA` if the server has IPv6) pointing at the server. Keep it
**DNS-only**: if you use Cloudflare, do not turn on the orange-cloud proxy,
or Cloudflare terminates TLS in front of you and sees every request.

## 2. Server

The smallest plan at any reputable provider is enough: 1 vCPU, 1 GB RAM,
20 GB disk, Ubuntu 24.04 LTS. Create it with your SSH key, not a password.
Turn on the provider's firewall and allow only 22, 80 and 443.

First login, as root:

```bash
apt update && apt upgrade -y
apt install -y unattended-upgrades ufw
dpkg-reconfigure -plow unattended-upgrades
ufw allow OpenSSH && ufw allow 80/tcp && ufw allow 443/tcp && ufw --force enable
sed -i 's/^#\?PasswordAuthentication .*/PasswordAuthentication no/' /etc/ssh/sshd_config
systemctl restart ssh
useradd --system --home /var/lib/rayhunter-collector --shell /usr/sbin/nologin rayhunter
mkdir -p /var/lib/rayhunter-collector/{data,keys} /var/www/rayhunter-data
chown -R rayhunter:rayhunter /var/lib/rayhunter-collector /var/www/rayhunter-data
chmod 700 /var/lib/rayhunter-collector/keys
```

## 3. Caddy

Install Caddy from its official repository (see caddyserver.com/docs/install),
then:

```bash
cp Caddyfile /etc/caddy/Caddyfile      # edit the domain first
systemctl reload caddy
```

Caddy obtains the certificate from Let's Encrypt on its own once DNS
resolves to the server. The `Caddyfile` discards access logs on purpose:
the service is designed not to keep client addresses, and the proxy must
not undo that.

## 4. The binary

Build a static Linux binary on your laptop and copy it up:

```bash
rustup target add x86_64-unknown-linux-musl
cargo build --release -p rayhunter-collector --target x86_64-unknown-linux-musl
scp target/x86_64-unknown-linux-musl/release/rayhunter-collector root@data.example.org:/usr/local/bin/
```

The repository's `.cargo/config.toml` already links that target statically,
so the binary has no dependencies on the server.

## 5. Keys, on the laptop

```bash
rayhunter-collector keygen --out ~/rayhunter-keys
scp ~/rayhunter-keys/{ingest.key,ingest.pub,archive.pub} root@data.example.org:/var/lib/rayhunter-collector/keys/
ssh root@data.example.org 'chown rayhunter:rayhunter /var/lib/rayhunter-collector/keys/* && chmod 600 /var/lib/rayhunter-collector/keys/*'
```

`~/rayhunter-keys/archive.key` stays on the laptop. Back it up to an
encrypted USB stick kept somewhere else, together with the restic passphrase
from step 8. Losing it means every full-tier capture ever received is gone
for good; leaking it means every one of them is readable by whoever has it.

Publish the two fingerprints that `keygen` printed on the site's about page
so owners can compare them with what their device shows.

## 6. The service

```bash
cp rayhunter-collector.service rayhunter-collector-publish.service rayhunter-collector-publish.timer /etc/systemd/system/
# edit --name, --contact, --site-url, and add --accept-full only if you want full submissions
systemctl daemon-reload
systemctl enable --now rayhunter-collector.service rayhunter-collector-publish.timer
curl -s https://data.example.org/.well-known/rayhunter-telemetry
```

The last line should print the service's description with your name and
keys. That is what a unit reads when its owner presses *Check server*.

## 7. The about page

Copy `about.html` into `/var/www/rayhunter-data/` and edit it. `publish`
rewrites the list, map, data and files, and leaves other files alone. Say
who you are, how to reach you, what you keep and do not, and how to
withdraw. Owners are trusting a person, and the page is where they find out
who.

## 8. Backups

Only `/var/lib/rayhunter-collector/data` matters; everything else is
reproducible from this runbook. Nightly, encrypted, off the machine:

```bash
apt install -y restic
restic -r b2:your-bucket:collector init          # once; pick a strong passphrase, keep it with archive.key
restic -r b2:your-bucket:collector backup /var/lib/rayhunter-collector/data
restic -r b2:your-bucket:collector forget --keep-daily 14 --keep-weekly 8 --prune
```

Put the last two lines in a root cron job or systemd timer with the B2
credentials in an environment file. Restore once to prove it works.
Backblaze B2 stores the first 10 GB free and about $7 per TB after that.

## 9. Monitoring

- An uptime check (UptimeRobot's free plan is enough) on
  `https://data.example.org/.well-known/rayhunter-telemetry`, alerting you by
  email.
- `df -h /` in the same cron job as the backup, mailing you above 80 %.
  The service stops accepting at `--max-disk-gb`, so a full disk is a
  visible refusal rather than a crash.
- `journalctl -u rayhunter-collector -f` when something looks wrong. It logs
  submission ids and key ids, never addresses.

## 10. Reviewing

```bash
ssh data.example.org
sudo -u rayhunter rayhunter-collector list --data /var/lib/rayhunter-collector/data --status received
sudo -u rayhunter rayhunter-collector show --data /var/lib/rayhunter-collector/data <id>
sudo -u rayhunter rayhunter-collector review --data /var/lib/rayhunter-collector/data <id> --status verified --tag interesting --note "..." --reviewer you
```

The publish timer picks the change up within 15 minutes, or run
`systemctl start rayhunter-collector-publish.service` at once. To look at a
full-tier capture, copy that submission's directory to the laptop
(`rsync -a data.example.org:/var/lib/rayhunter-collector/data/submissions/<id> .`)
and run `decrypt` there with `archive.key`.

## 11. Pointing a unit at it

On a Rayhunter: Settings → Community → tick *Contribute*, enter
`https://data.example.org`, *Check server*, compare the fingerprints, *Pin
these keys*, save. Its first upload arrives an hour after the next warning
closes, once it is on WiFi.

## If something goes wrong

- **The server is compromised.** Assume `ingest.key` is taken: it opens
  summaries sent from now on, not past captures. Rebuild the server from this
  runbook, run `keygen` again, publish the new fingerprints, and tell owners
  to press *Check server* and pin again; until they do, their units refuse
  to send. `archive.key` was never there, so nothing raw was exposed.
- **`archive.key` is lost.** Full-tier captures already received are
  unreadable forever. Generate a new pair, publish the new fingerprint, and
  owners re-pin. Summaries are unaffected.
- **A takedown or removal request.** `review --status rejected` removes an
  entry from the next publish. A withdrawal from the unit itself deletes
  the payload immediately.
- **Abuse.** Add the submitter key id (from `list`) to
  `/var/lib/rayhunter-collector/data/bans.json` as a JSON list of strings
  and restart the service.

## Cost, roughly

| Item | A year |
|---|---|
| Domain (`.org` at cost) | $9 to $12 |
| Smallest VPS at DigitalOcean, Hetzner, Linode or Vultr | $48 to $72 |
| Provider backups or snapshots (optional) | $10 to $20 |
| Backblaze B2 for off-machine backups | $0 to $5 at this size |
| Certificates, DNS, uptime check | $0 |

About $60 to $110 a year. Bandwidth is not a concern: a summary is a few
megabytes, a full capture at most a few hundred, and every plan above
includes hundreds of gigabytes a month.
