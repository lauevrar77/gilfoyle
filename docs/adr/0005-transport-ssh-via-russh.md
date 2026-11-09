# Transport MCP over SSH with an embedded russh server

Status: accepted

Gilfoyle's only transport is SSH, served in-process by `russh`; there is no HTTP. A client
opens a connection, authenticates by public key, and requests the `gilfoyle-mcp` subsystem;
that channel carries newline-delimited MCP JSON-RPC (rmcp's async-rw transport, fed from
`channel.into_stream()`). The handler accepts only that subsystem and refuses
`shell`/`exec`/`pty` and every forwarding request, so SSH is a protocol, never a shell.

Considered options: keeping Streamable HTTP (rejected — no host identity, a bearer token to
protect, and no reuse of existing SSH infrastructure); OpenSSH with a forced command or
subsystem (rejected — the "no shell" property would live in sshd configuration rather than
in the code).

Consequences: the workspace gains a C dependency — the SSH crypto backend
(`ring`) — and a large pre-authentication parser surface that HTTP did not have;
`unsafe` now exists transitively (`russh-cryptovec`, the backend). The "pure Rust, no C"
posture of ADR 0001 no longer holds, and the ships-a-static-musl-binary release format is
dropped in favour of a dynamically linked GNU build. This is a deliberate trade: it makes
"SSH is not a shell" an invariant of the code, at the cost of the parser surface and the C
dependency.
