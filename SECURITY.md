# Security Policy

## Supported Versions

Only the latest tagged release receives security fixes. Users should reproduce
an issue on the latest release before reporting it when this can be done
without exposing sensitive data.

## Reporting A Vulnerability

Use GitHub's private vulnerability reporting feature for this repository. Do
not open a public issue, discussion, or pull request for a suspected security
vulnerability.

Include the affected version, deployment topology, reproduction steps, impact,
and any suggested mitigation. Remove credentials, personal data, access tokens,
database dumps, and production logs from the report.

Receipt should be acknowledged within seven days. Validation, disclosure, and
release timing depend on severity and reproducibility. Please allow a fix to be
released before publishing technical details.

## Deployment Responsibility

Hegira is an application framework, not a managed service. Operators of generated
applications remain responsible for secret management, TLS termination, network policy,
database access, backup/restore testing, dependency updates, and validation of
optional external providers in their own environment.
