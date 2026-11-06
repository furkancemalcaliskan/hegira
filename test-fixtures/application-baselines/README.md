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
directory and review a recursive comparison. Preserve previously frozen
release content.

## v0.7.0 provenance and replay

The `v0.7.0` set was generated from the signed, immutable
[published release](https://github.com/furkancemalcaliskan/hegira/releases/tag/v0.7.0).
Its tag object is `f83979ef149eb1ed0017edee143cd502c070ce07`, commit is
`635c163d50375f7d250311426c67413da382ed23`, and source tree is
`2547cb85aae856024771cbe4432a2a88e4703574`. The source package digest is
`sha256:34ddf6356d441791c93f306f5ea6aeb5927cab1062d2e6d7f8b8928ee198b053`;
the generated framework dependency revision remains the released
`7a9ed5bce734f5459fee62d3a2069ce1cfc7d212`.

The maintainer-only importer requires Git with SSH signature verification,
Cargo and the released Rust toolchain. It pins the reviewed release signing
public key, verifies the annotated tag/commit/tree and package identity, archives
that exact source, builds its locked CLI, and renders all six compositions:

```sh
sh scripts/import-v0.7.0-application-baselines.sh . /tmp/v0.7.0-baselines
diff -r test-fixtures/application-baselines/v0.7.0 /tmp/v0.7.0-baselines
```

The destination must be absent under an existing canonical parent outside this
repository. Build/render state uses the separately owned `baseline-import-v0-7-0`
validation cache and its normal budget and cleanup. No rendered application,
provider, migration, seed, HTTP service, or frontend hook is executed.
Identity addition uses the released mutation publisher and then the same provider's
released default lock bytes, following the existing v0.6.0 convention. This exact
selection is recorded in `provenance.toml`; no application dependency resolution
or historical lock repair is performed.

Both releases have code-pinned inventories plus sorted, digest-authenticated
trees and shared content objects. The v0.7.0 inventory additionally authenticates
its provenance file. Modified input cannot become a new trusted baseline by
rehashing the object, tree and release manifest together. Unknown entries,
release selections and symlinks fail closed. The data packer verifies its result
against the pinned catalog before publication succeeds and refuses existing
destinations. Replaying frozen snapshots needs neither Git nor the network.

Select v0.7.0 explicitly with
`BaselineCatalog::from_repository_release(root, BaselineVersion::V0_7_0)`.
Existing catalog entry points continue to select v0.6.0. The required
`sh scripts/cli-check.sh` gate tests both versions and the data packer's offline
replay/tampering cases. The current public source-upgrade edge is
v0.7.0 to v0.8.0; baseline selection belongs to test support. The archived
v0.6.0-to-v0.7.0 edge and both published baseline inventories remain immutable.
