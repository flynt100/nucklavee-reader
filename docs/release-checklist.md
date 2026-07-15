# Experimental MVP release checklist

## Code quality

- [ ] Formatting and Clippy pass with warnings denied.
- [ ] Full locked test suite and release build pass.
- [ ] `cargo package --locked` succeeds and package contents are inspected.
- [ ] No ignored test conceals a release blocker.

## Security and privacy

- [ ] Dependency advisories and licenses are reviewed.
- [ ] No credentials or generated private data are committed.
- [ ] URL size and private-network policy tests pass.
- [ ] Embedding data-transfer documentation is current.
- [ ] `SECURITY.md` reflects supported deployment scope.

## Documentation and distribution

- [ ] README capabilities and limitations match the code.
- [ ] Installation and config steps work in a clean environment.
- [ ] Version and changelog are current.
- [ ] Tag is attributable and named `v0.1.0`.
- [ ] GitHub release notes lead with experimental status and limitations.
- [ ] Binary checksums are provided if binaries are distributed.

## Post-release

- [ ] CI is green on the tag and a fresh installation is verified.
- [ ] Deferred work is represented by roadmap issues.
- [ ] Security and bug reports are monitored.
