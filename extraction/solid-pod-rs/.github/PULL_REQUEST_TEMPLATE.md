<!--
Thanks for contributing to solid-pod-rs!
Please fill in the sections below. Delete any that do not apply.
-->

## Summary

<!-- One or two sentences explaining what this PR does and why. -->

## Related issues

<!-- Link to issues: "Closes #123", "Refs #456". -->

## Type of change

- [ ] Bug fix (non-breaking change that resolves an issue)
- [ ] New feature (non-breaking change that adds functionality)
- [ ] Breaking change (fix or feature that changes public API)
- [ ] Documentation update
- [ ] CI / tooling / build
- [ ] Parity fix (behaviour alignment with reference Solid server)

## Checklist

- [ ] `cargo fmt --all -- --check` passes
- [ ] `cargo clippy --all-targets --all-features -- -D warnings` passes
- [ ] `cargo test --all-features` passes locally
- [ ] `cargo doc --no-deps` produces no broken intra-doc links
- [ ] Added or updated unit / integration tests for the change
- [ ] Updated `CHANGELOG.md` under the `[Unreleased]` section
- [ ] Updated `README.md` / rustdoc where public API changed
- [ ] No new `unwrap`/`expect` in non-test code without justification

## Parity checklist (if applicable)

<!-- Tick these only when the PR changes user-visible behaviour compared to JSS. -->

- [ ] Behaviour verified against the reference Solid server
- [ ] `PARITY-CHECKLIST.md` updated (entry added, moved, or closed)
- [ ] Interop regression test added under `tests/interop_jss.rs` or similar

## Security impact

<!-- Does this change touch auth, WAC, OIDC, or input parsing? If so, describe the threat model delta. -->

## Performance impact

<!-- Benchmark numbers or "no measurable impact" with rationale. -->

## Notes for reviewers

<!-- Anything reviewers should focus on. -->
