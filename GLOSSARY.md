# Gilfoyle

Gilfoyle is a secure MCP server that administers a Linux host through narrow, allowlisted
capabilities. This glossary fixes the language of that domain.

## Topology

**Node**:
A single Gilfoyle process on one host; it serves that host and exposes tools over SSH.
_Avoid_: server, agent, host, machine

**Node label**:
The name a Client gives a Node; it is the value of a tool call's `node` argument.
_Avoid_: machine name, alias, hostname

**Client**:
The aggregator that holds connections to one or more Nodes and presents a single MCP surface.
_Avoid_: gateway, hub, proxy, server

**Agent**:
The AI that consumes MCP and decides which tools to call. Never a Node.
_Avoid_: assistant, model, bot

**Session**:
One subsystem channel between a Client and a Node, carrying one MCP conversation. A connection may carry several.
_Avoid_: connection, stream

## Host and units

**Unit**:
A systemd object that is managed as one thing, addressed by a name whose suffix names its
type (`nginx.service`, `sshd.socket`).
_Avoid_: daemon, process

**Service**:
A unit of type `.service`. A subtype of Unit, not a synonym for it.
_Avoid_: unit (when a service specifically is meant)

**Unit name**:
The fully-qualified identifier of a unit, including its type suffix (`nginx.service`). A
short name such as `nginx` is not a unit name until it is normalised.
_Avoid_: service name

**Active state**:
The lifecycle state of a unit: `active`, `inactive`, `failed`, `activating`,
`deactivating`, `reloading`, `maintaining`.
_Avoid_: status, state

**Load state**:
Whether systemd could load a unit's definition: `loaded`, `error`, `masked`.
_Avoid_: —

**Sub state**:
The unit-type-specific refinement of the active state (for example `running`, `dead`,
`exited`).
_Avoid_: —

## Operations

**Job**:
The asynchronous operation systemd enqueues when a unit is started or stopped. It is
tracked by a job path and finishes with a job result.
_Avoid_: task, request

**Job result**:
The outcome of a job: `done`, `failed`, `canceled`, `timeout`, `dependency` or `skipped`.
Only `done` is success.
_Avoid_: status

**Managed unit**:
A unit listed in `[systemd] manage_units`. Only managed units may be started or stopped.
_Avoid_: allowed service

**Allowlist**:
The set of managed units. It is empty by default, so every start and stop is refused until
the operator opts in.
_Avoid_: whitelist, permitted list

## Packages

**Package**:
A unit of software that a package manager installs, updates and removes.
_Avoid_: program, library

**Package id**:
The identifier of a package across repositories: its name, version, architecture and
origin, carried as one string.
_Avoid_: package name

**Installed package**:
A package present on the host, with the version currently installed.
_Avoid_: package (when an installed one specifically is meant)

**Update**:
A newer version of an installed package, available from a repository.

**Upgrade**:
Applying the available updates to installed packages. It never installs a package that
was not already present, and never removes one.
_Avoid_: update (as a verb)

**Refresh**:
Fetching repository metadata so that updates can be discovered. It changes no package.

**Managed operation**:
A mutating package operation listed in `[packagekit] allow`. Only `refresh` and
`upgrade` exist; the allowlist is empty by default.
_Avoid_: allowed operation

**Trusted package**:
A package signed by a repository key the host trusts. Gilfoyle acts only on trusted
packages.
_Avoid_: signed package

**Untrusted package**:
A package that is unsigned or cannot be verified. Gilfoyle always refuses it, and never
trusts a new signing key.
_Avoid_: unsigned package

## Trust

**Principal**:
The authenticated identity of a connection: the id of the key that signed it.
_Avoid_: user, token, account

**Host key**:
The private key a Node proves its identity with; a Client pins its fingerprint.
_Avoid_: server key, machine key

**Authorized key**:
A client public key a Node accepts, optionally limited to a set of tools.
_Avoid_: token, credential, API key

## Measurements

**Uptime**:
The time elapsed since the host kernel booted (read from `/proc/uptime`), not the age of
the gilfoyle process.
_Avoid_: process uptime
