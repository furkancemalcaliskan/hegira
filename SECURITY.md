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

## Local Tool And Application Trust

CLI previews, inspection and source upgrades do not grant permission to execute
application code, access databases or deploy. Doctor's optional native probes
run explicitly selected trusted tools; they do not execute application hooks.
See the [workflow boundaries](docs/getting-started.md#choose-an-application-workflow).

Explicit execution trusts application source, Cargo configuration, build scripts,
tests, installed tools and the inherited environment with the caller's privileges.
It is not a sandbox. Even a database status command can compile trusted code and
write Cargo output. Review runtime settings and URL overrides before approving
database access; production migration requires separate CLI approval, and an
interrupted migration may have changed the target.

Typed content-redacted reports do not sanitize arbitrary application behavior.
Human dev/check/test/build execution inherits raw child logs; their JSON mode
discards child output. Database execution discards raw logs in both modes and
accepts only its bounded application protocol. Keep secrets out of logs, browser
assets and validation artifacts. See [operation output and trust](docs/getting-started.md#check-and-test-an-application)
and [database semantics](docs/operations.md#explicit-cli-database-operations).

## Deployment Responsibility

Hegira is an application framework, not a managed service. Operators of generated
applications remain responsible for secret management, TLS termination, network policy,
database access, backup/restore testing, dependency updates, and validation of
optional external providers in their own environment.
