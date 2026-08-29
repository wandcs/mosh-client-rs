# Dependency policy

Add a dependency only when it solves a defined protocol, security, portability,
or verification problem better than small local code.

Before adoption, record:

- exact purpose and rejected alternatives;
- SPDX license and compatibility with `MIT OR Apache-2.0`;
- maintenance and security history;
- transitive dependency count and licenses;
- `unsafe` code and native build requirements;
- support for Linux test hosts and ARM64 HarmonyOS targets;
- input limits, failure behavior, and removal plan.

Cryptographic algorithms, secure random generation, secret zeroization, and
Unicode width tables should come from mature, audited, permissively licensed
libraries. No dependency may silently introduce GPL, AGPL, proprietary code,
network services, telemetry, or runtime downloads.
