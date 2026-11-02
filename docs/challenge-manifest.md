# Challenge manifest specification

Status: draft.

`challenge.yaml` describes a challenge's metadata, flags, participant files,
runtime services, static file generation, and containerized tests.

The manifest is declarative. Dockerfiles contain compilation, generation,
packaging, and test-tool installation logic. Challenge-specific interpreters,
compilers, archive tools, and solver libraries run inside containers.

## 1. Top-level structure

```yaml
name: example
fullName: Example Challenge
description: An example challenge.
categories: [misc]
tags: []
authors: []
instancing: true

flags:
  - name: main
    body: example-only

files: []
services: []
build: []
test: []
```

| Field | Type | Purpose |
|---|---|---|
| `name` | String | Machine-readable challenge identifier. |
| `fullName` | String | Display name. |
| `description` | String | Challenge description. |
| `categories` | List of strings | Category metadata. |
| `tags` | List of strings | Additional classification metadata. |
| `authors` | List of strings | Challenge authors. |
| `flags` | List of objects | Named flag definitions. |
| `instancing` | Boolean | `true` for on-demand runtime instances; `false` for shared runtime. |
| `instanceDurationSeconds` | Integer | Optional instance duration. |
| `files` | List of strings | Participant-facing file references. |
| `services` | List of objects | Runtime service definitions. |
| `build` | List of objects | Static file generators. |
| `test` | List of objects | Test runner definitions. |

`services`, `build`, and `test` are lists. Each entry has a `name`, unique within
that section. They are not maps keyed by names.

Capabilities are independent:

- Static files need no generator.
- Generated files need no hosted service.
- Services need no solver.
- Tests may consume files, service endpoints, or both.
- Omitted or empty `build` and `test` sections declare no corresponding work.
- Omitted or empty `services` declares no hosted runtime.
- Absence of tests means **not tested**, not **passed**.

Categories do not select an execution model. For example, an offline application
can be represented by generated files and a file-consuming test.

## 2. Image definitions

Service, static-generation, and test entries use an `image` object.

### 2.1 Dockerfile source

```yaml
image:
  type: dockerfile
  context: .
  path: source/Dockerfile
  target: export
```

| Field | Meaning |
|---|---|
| `type` | `dockerfile`. |
| `context` | Docker build context directory. Relative to the directory containing `challenge.yaml`. |
| `path` | Dockerfile path. Relative to the selected context. |
| `target` | Optional Docker stage name. Omission selects the final stage. |

When `context` is omitted it defaults to `.`, the directory containing
`challenge.yaml`.

A context of `.` gives Docker access to the challenge directory, subject to
`.dockerignore`. A context of `source` restricts ordinary `COPY` inputs to that
directory. A sibling directory outside the context is not implicitly available.

`target` selects a Docker stage. It is not a source directory, destination
directory, or artifact type.

```dockerfile
FROM alpine AS generate
RUN mkdir /out && printf 'example\n' > /out/data.txt

FROM generate AS package
RUN tar -czf /out/package.tar.gz -C /out data.txt

FROM scratch AS export
COPY --from=package /out/package.tar.gz /package.tar.gz
```

Selecting `target: export` builds the stages needed by `export`. Dependencies
follow Docker's `FROM` and `COPY --from=` semantics. Unrelated stages need not
execute.

Stage names are arbitrary. There is no required stage name or output directory.

### 2.2 Registry source

Services may reference an image directly:

```yaml
image:
  type: image
  image: registry.example.org/team/service:version
```

Support for registry-sourced static generators and test runners is not defined
in this draft.

### 2.3 Purpose of the resulting image

| Section | Image use |
|---|---|
| `services` | Run the hosted service. |
| `build` | Export generated files from the selected filesystem. |
| `test` | Execute the test process. |

`image` describes the source of the filesystem or executable container.
Runtime mounts are separate from image construction.

## 3. Services

```yaml
services:
  - name: server
    image:
      type: dockerfile
      context: source/server
      path: Dockerfile
      target: runtime
    env:
      - name: MODE
        value: challenge
    flags:
      - name: main
        file: /app/flag.txt
    ports:
      - name: tcp
        containerPort: 1337
        expose: true
        type: netcat

  - name: worker
    image:
      type: dockerfile
      context: source/worker
      path: Dockerfile
```

| Field | Purpose |
|---|---|
| `name` | Service identifier. |
| `image` | Dockerfile or registry image source. |
| `env` | Environment entries containing `name` and `value`. |
| `flags` | Flag bindings identifying a named flag and its environment variable or file destination. |
| `ports` | Named ports with `containerPort`, `expose`, and `type`. |

A flag binding uses `env` for an environment variable or `file` for a file path:

```yaml
flags:
  - name: main
    env: FLAG
```

Port types are `http`, `https`, `netcat`, `ssh`, and `stty`.
`containerPort` is the application's internal listening port. It is not
necessarily the externally allocated port.

A service's image definition is sufficient to identify its runtime image build.
It must not be duplicated under `build`.

Service runtime volumes are not defined by the test mount specification.

## 4. Static generation

`build` defines named static generators:

```yaml
build:
  - name: prepare
    image:
      type: dockerfile
      context: .
      path: source/generator/Dockerfile
      target: export

  - name: package
    image:
      type: dockerfile
      context: source/package
      path: Dockerfile
```

| Field | Purpose |
|---|---|
| `name` | Identifier used in generated file references. |
| `image` | Dockerfile source for the resulting filesystem. |

Generation happens during the Docker build. The resulting image's
`ENTRYPOINT` and `CMD` are not run to generate its files.

The selected filesystem may contain multiple files in multiple directories.
There is no intermediate named-artifact map or `output` field.

Dockerfiles may compile binaries, generate data, assemble public sources, and
create archives. Archive creation is not inferred from a selected directory.

Multiple generators may use different Dockerfiles, or the same Dockerfile with
different targets.

A build entry's `name` does not automatically make it available as a Docker
`FROM` source in another entry. Cross-entry dependency bindings are not defined.

## 5. Participant files and path references

### 5.1 File selection

```yaml
files:
  - public/README.md
  - build://prepare/out/program
  - build://prepare/packages/source.zip
  - build://package/other/directory/data.bin
```

`files` selects participant-facing files. Producing a file or mounting it into a
test does not automatically select it here.

An ordinary relative path refers to a host file relative to the directory
containing `challenge.yaml`.

A generated reference has this form:

```text
build://<build-name>/<path-in-result>
```

The build name selects a `build` entry. The remaining path starts at that
entry's selected filesystem root.

| Reference | Meaning |
|---|---|
| `public/README.md` | `<challenge-directory>/public/README.md` |
| `build://prepare/out/program` | `/out/program` in build `prepare` |
| `build://prepare/packages/source.zip` | `/packages/source.zip` in build `prepare` |
| `build://package/other/directory/data.bin` | `/other/directory/data.bin` in build `package` |

Generated paths are never relative to Dockerfile `WORKDIR`, the build context,
or a host export directory.

### 5.2 Path domains

| Path | Base |
|---|---|
| `image.context` | Directory containing `challenge.yaml`. |
| Dockerfile `image.path` | Selected context. |
| Ordinary relative file or mount source | Directory containing `challenge.yaml`. |
| `build://name/path` | Root of the named build's selected filesystem. |
| Test mount `target` | Absolute destination inside the test container. |

A leading `/` on an ordinary host path means an absolute host path. This is
distinct from the root of a generated filesystem.

Generated references contain no host storage destination.

### 5.3 Reference validation

Build names must resolve to declared entries.

Generated files need not exist before generation. File existence is checked
after the producing build succeeds. A failed producer or missing selected path
is an error, not an empty successful result.

## 6. Tests

```yaml
test:
  - name: check-files
    image:
      type: dockerfile
      context: solution
      path: Dockerfile
      target: files-test
    mounts:
      - source: build://prepare/out/program
        target: /inputs/program

      - source: public/reference.txt
        target: /inputs/reference.txt

  - name: solve
    image:
      type: dockerfile
      context: solution
      path: Dockerfile
      target: service-test
```

| Field | Purpose |
|---|---|
| `name` | Test identifier. |
| `image` | Runner image source. |
| `mounts` | Optional list of runtime input bindings. |

The image's `ENTRYPOINT` and `CMD` define the test process. The image provides its
own language runtime, dependencies, assertions, and solver logic.

All test runners execute externally in Docker. They never run as Kubernetes
Jobs or other in-cluster workloads.

### 6.1 Input mounts

```yaml
mounts:
  - source: build://prepare/out/program
    target: /inputs/program

  - source: public/reference.txt
    target: /inputs/reference.txt
```

- Mounts are a list of explicit `source`/`target` objects.
- `source` follows the reference rules in section 5.
- A source can be a file or directory.
- `target` is an absolute runner-container path.
- Mounts are read-only by default.
- Generated inputs must exist before the test runs.
- Mounts apply when executing the runner, not when building its image.
- A test input does not have to appear in participant-facing `files`.

### 6.2 Runtime availability

Runtime availability is derived from the challenge, not from an individual
test's configuration.

| Challenge configuration | Runtime available to every test |
|---|---|
| Services and `instancing: true` | An on-demand runtime and its endpoints. |
| Services and `instancing: false` | The shared runtime and its endpoints. |
| No services | No service runtime or endpoint variables. |

Every test of a hosted challenge receives endpoint variables alongside its
declared file mounts. A runner may ignore endpoints if it only checks files.

There is no per-test `instance` field. `instancing: false` means shared runtime,
not an offline challenge. `instancing: true` without services is invalid.

Runners execute in Docker, never inside the cluster:

- Runners of hosted challenges join the cluster's Docker network and reach
  services through their endpoint variables only.
- Runners of challenges without services have no network access. Every
  dependency must be part of the runner image or a declared mount.
- A runner starts only after every exposed endpoint of its runtime accepts
  TCP connections.

### 6.3 Results

- Exit code `0` means the declared test passed.
- A nonzero exit means it failed.
- Failure to build, prepare the runtime, or start the runner is distinct from
  an assertion failure.
- An absent or empty `test` section means **not tested**.

## 7. Test environment variables

### 7.1 Service endpoints

Endpoint variables follow:

```text
SERVICE_<SERVICE_NAME>_<PORT_NAME>_<FIELD>
```

Service and port tokens are uppercase, with hyphens replaced by underscores.
Normalization collisions must be rejected.

| Field | Value |
|---|---|
| `HOST` | Externally reachable hostname/address, without a scheme or port suffix. |
| `PORT` | Externally reachable port as a decimal string. |
| `ENDPOINT` | Combined address appropriate to the endpoint type. |

```text
SERVICE_SERVER_TCP_HOST=dev-host.example.test
SERVICE_SERVER_TCP_PORT=32001
SERVICE_SERVER_TCP_ENDPOINT=dev-host.example.test:32001

SERVICE_WEB_HTTP_HOST=dev-host.example.test
SERVICE_WEB_HTTP_PORT=32002
SERVICE_WEB_HTTP_ENDPOINT=http://dev-host.example.test:32002
```

The example addresses and ports are illustrative. Values must be reachable from
the external runner. Kubernetes-only service DNS and internal container ports
must not be substituted for externally reachable endpoints.

There is no single-endpoint alias, even for a service with one port.
For example, `SERVICE_SERVER_ENDPOINT` is not injected.

### 7.2 Flags

Flags use one environment variable per flag:

```text
flag_<flag_name>
```

```text
flag_main=synthetic-test-flag
```

The prefix is lowercase `flag_`. There is no service or challenge prefix and no
JSON flag map. The value is the flag the runner is expected to recover.

A flag's expected value depends on whether a service receives it:

| Flag binding | Expected value |
|---|---|
| Not referenced by any service `flags` entry (static) | The literal flag. Static flags are baked into images or files and must not declare `random`. |
| Injected, `instancing: false` | The literal flag plus the flag's own `random` suffix, if declared. |
| Injected, `instancing: true` | As above, additionally applying the event's default `random` rules. |

For injected flags the value is the one the runtime actually received.

## 8. Complete example

```yaml
name: example
fullName: Example Challenge
categories: [misc]
authors: []
instancing: true

flags:
  - name: main
    body: example-only

files:
  - public/README.md
  - build://prepare/out/program
  - build://prepare/packages/source.zip

services:
  - name: server
    image:
      type: dockerfile
      context: source/server
      path: Dockerfile
      target: runtime
    flags:
      - name: main
        file: /app/flag.txt
    ports:
      - name: tcp
        containerPort: 1337
        expose: true
        type: netcat

build:
  - name: prepare
    image:
      type: dockerfile
      context: .
      path: source/generator/Dockerfile
      target: export

test:
  - name: check-files
    image:
      type: dockerfile
      context: solution
      path: Dockerfile
      target: files-test
    mounts:
      - source: build://prepare/out/program
        target: /inputs/program

  - name: solve
    image:
      type: dockerfile
      context: solution
      path: Dockerfile
      target: service-test
    mounts:
      - source: build://prepare/out/program
        target: /inputs/program
```

`prepare` generates static files. `server` defines the hosted service.
Both tests receive the runtime's external endpoints because the challenge has
services and `instancing: true`; both also receive their declared file mounts.

Directory and stage names are explicit configuration, not reserved conventions.

## 9. Unspecified details

The following details are outside the defined draft contract:

| Area | Unspecified behavior |
|---|---|
| Image sources | Registry images for static generators and test runners. |
| Build dependencies | Bindings between separate static build entries. |
| File selection | Directory entries in `files`, filename overrides, and duplicate final filenames. |
| References | Escaping, traversal rejection, symlinks, and portability rules for absolute host paths. |
| Test execution | Command overrides, writable mounts, and additional mount options. |
| Endpoints | Endpoint formatting for every port type, HTTPS setup, and IPv6 formatting. |
| Runtime lifecycle | Isolation of a shared test runtime between tests. |
