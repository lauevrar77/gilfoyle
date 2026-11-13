# Hold the mutation score at 100%

The workspace gates on `cargo-mutants` with a 100% score: a single surviving mutant fails
the build. This is affordable because the domain core is pure and the adapters are thin,
and it is worth it for a security-sensitive server, where untested branches are exactly
where holes hide.

The composition root (`main.rs`) is the only exclusion. `#[mutants::skip]` is reserved for
I/O that cannot be simulated, always with a written justification.

The score is unchanged by *where* the gate runs; only the CI cost is. The whole workspace is
617 mutants, which is too slow for every push, so CI splits it: an **incremental** run
(`--in-diff` over a pull request's changed lines) on each PR, and the **full** workspace run
in **8 shards** (`--shard k/8`) weekly and on manual dispatch. `--in-place`, used locally by
`make check`, keeps disk usage low but disables parallelism, so the local run stays serial;
CI sets `MUTANTS_IN_PLACE=0` to shard and parallelise. The 100% bar applies to both. The
full gate is still the one that must pass before a push — the incremental gate only sees the
lines in the diff.
