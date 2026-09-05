# Security Policy

## Reporting a Vulnerability

**This is a fork of [EFForg/rayhunter](https://github.com/EFForg/rayhunter), and where
you should report depends on which code the problem is in.**

If the vulnerability is in code this fork adds or changes — the pairing and TLS
setup flow, the auth store under `/data/rayhunter/auth/`, the web accounts, the
terminal feature, or any of the fork-only API endpoints — report it privately
here, using GitHub's [private vulnerability reporting tool on this
repository](https://github.com/holdTheDoorHoid/rayhunter/security/advisories/new).

If it is in code this fork inherits unchanged from upstream, it affects every
Rayhunter user, so report it to EFF using their [private vulnerability reporting
tool](https://github.com/EFForg/rayhunter/security/advisories/new). If you are
not sure which applies, report it here and it will be forwarded.

Please do not open a public issue for a vulnerability.

## Scope

Rayhunter runs as root on a mobile hotspot that is, by design, reachable from
every client on that hotspot's network. Reports about the device's exposed
surface — the web UI, the API, the pairing flow, the recordings on disk — are in
scope. The carrier firmware Rayhunter is installed onto is not: report those to
the device vendor.
