# Hold the mutation score at 100%

The workspace gates on `cargo-mutants` with a 100% score: a single surviving mutant fails
the build. This is affordable because the domain core is pure and the adapters are thin,
and it is worth it for a security-sensitive server, where untested branches are exactly
where holes hide.

The composition root (`main.rs`) is the only exclusion. `#[mutants::skip]` is reserved for
I/O that cannot be simulated, always with a written justification.
