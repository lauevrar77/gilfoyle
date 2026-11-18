# Reach nodes through a jump host over `direct-tcpip`

Status: accepted

A client may need to reach a node it cannot open a TCP connection to — the node is on a
private network, behind a firewall, or only routable from a bastion. The client therefore
accepts an optional **jump host**: it opens an SSH connection to the jump host,
authenticates by public key, opens a `direct-tcpip` channel to the node's `host:port`
(RFC 4254 §7), and runs the node's SSH session **over that channel**
(`russh::client::connect_stream`). This is `ProxyJump`, done in-process: no `ssh`, no
`ProxyCommand`, no subprocess. The connection to the jump host is kept alive for as long as
the node's sessions live, because the node's transport rides on one of its channels.

## The jump host must be an OpenSSH sshd, and only relays bytes

The jump host must support TCP forwarding (`AllowTcpForwarding`, the OpenSSH default). A
gilfoyle node **cannot** be a jump host: its handler refuses every forwarding request, so it
rejects the `direct-tcpip` channel — a fact now pinned by a test. The client reports a clear
error when the channel is refused.

The jump host must **forward bytes, never terminate SSH**. The session to the node stays
end-to-end encrypted through the tunnel, so the jump host cannot read the MCP traffic, and
the node's host-key fingerprint is pinned as usual, so it cannot impersonate the node
either. A jump host that terminated the client's SSH and re-originated it would defeat both
properties and would make the node see the jump host's identity instead of the client's,
breaking the per-key tool grants. The documented model is a byte-forwarding relay.

## Configuration

```toml
[[jumps]]
name = "bastion"
host = "bastion.example.com"
port = 22
user = "laurent"
key = "~/.ssh/gilfoyle_bastion"
host_key_fingerprint = "SHA256:..."

[[nodes]]
label = "web1"
host = "10.0.0.5"       # resolved and reached from the jump host
port = 2222
key = "~/.ssh/gilfoyle_web1"
host_key_fingerprint = "SHA256:..."
jump = "bastion"        # optional; absent means a direct connection
```

A jump host has its own key and its own pinned fingerprint, and its own SSH user (a node's
label doubles as its user, but a jump host is a foreign host with its own account). A node
that references an unknown jump fails configuration; an unreferenced `[[jumps]]` entry is
allowed. The connection model is one SSH connection per node, so the reconnect behaviour of
the aggregation proxy is unchanged.

## Security

- **Confidentiality and identity are end-to-end.** The jump host relays ciphertext; the
  node's host key is pinned and its per-key grants still apply.
- **The jump host is a reachability path and an availability dependency**, not a trusted
  party for confidentiality. Compromising it lets an attacker refuse service, not read or
  forge the MCP session.
- The node's firewall and rate limiting now observe the jump host's address rather than the
  client's, and the jump host becomes an additional place to rate-limit.
- The jump host is an OpenSSH sshd, so its own shell/exec/forwarding surface is
  configuration-defined and outside gilfoyle's control. This does not weaken the *node's*
  "no shell" invariant, which stays in code; it only means the path now includes a
  shell-capable host that gilfoyle does not own.

## Considered options

- **Chained jumps** (`jump1 → jump2 → node`): out of scope. One hop covers the common case
  and keeps the configuration and the lifecycle simple.
- **`ProxyCommand` / `ssh -J` via a subprocess**: rejected. Gilfoyle never spawns a process
  (`clippy::disallowed_methods` rejects `std::process::Command` at compile time) and has no
  shell; `direct-tcpip` is the in-process equivalent.
- **A terminating proxy** (the jump host ends the client's SSH and re-originates it): rejected
  — it would break host-key pinning and the per-key grant model.

## Consequences

The client can reach nodes behind a bastion without a new binary, since the jump is
in-process: "nothing beyond the `gilfoyle` binary" still holds. A deployment that uses a jump
host needs an OpenSSH sshd with TCP forwarding, which the documentation states. A node that
is down at startup stays visible in `list_nodes` and errors on call, exactly as before.
