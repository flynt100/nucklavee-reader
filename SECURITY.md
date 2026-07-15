# Security policy

## Supported versions

Only the latest `0.1.x` revision is supported. Nucklavee is experimental and
is not currently supported as a hostile multi-user network service.

## Reporting a vulnerability

Use GitHub's private vulnerability reporting for this repository. Do not open
a public issue for an undisclosed vulnerability. Include reproduction steps,
affected versions, impact, and any suggested mitigation. The maintainer aims
to acknowledge reports within seven days and will coordinate disclosure after
a fix or mitigation is available.

## Security scope

Particularly relevant boundaries include URL fetching and redirects, local
file access, TOML/environment secrets, data sent to embedding providers,
SQLite migrations, and vector-index artifacts. URL ingestion blocks
non-public destinations and limits responses, but it remains intended for
trusted local use; DNS rebinding is a documented residual risk. Document
chunks and queries leave the machine when a remote embedding endpoint is used.

Do not include real credentials or sensitive documents in reports.
