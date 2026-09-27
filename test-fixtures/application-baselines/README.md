# Released application baselines

This directory stores immutable generated-application source states used by
upgrade tests. Baselines are data, not executable templates. Each release owns
one authenticated release manifest, sorted tree manifests, and a shared
content-addressed object store so identical files are stored only once.

The `v0.6.0` data was produced with the `hegira` binary built from the annotated
`v0.6.0` tag, then imported with:

```sh
sh scripts/import-v0.6.0-application-baselines.sh \
  /path/to/reviewed-v0.6.0-outputs \
  /tmp/v0.6.0-baselines
```

The reviewed outputs use the fixed application identity
`baseline-application` and cover default, minimal, and Identity-added states
for SQLite and PostgreSQL. Identity-added outputs begin as the released minimal
composition, apply the released `component add identity` plan, and use the
matching released default-composition lockfile because the additive command
does not rewrite `Cargo.lock`.

Normal tests read only the committed data through `upgrade_test_support`; they
do not invoke Git, access the network, execute fixture content, or regenerate a
released application from current templates. Re-import into an absent temporary
directory and review a recursive diff before intentionally updating any
baseline.
