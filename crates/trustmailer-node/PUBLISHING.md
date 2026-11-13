# Publishing @povio/trustmailer

Publish from `crates/trustmailer-node`, not the repository root or `target/release`.
The main package includes `trustmailer.*.node` directly. No separate `npm pack`
command or platform-package publishes are needed. The old `npm/` subdirectories
are not used by this fork's release workflow.

## First local release

The prepared version is `@povio/trustmailer@0.1.1`. The local build contains only
`trustmailer.darwin-arm64.node`, so this release supports Apple Silicon Macs only.
The npm account publishing it needs permission to create packages in `@povio`.

After building with the commands below, run from the repository root:

```sh
cd crates/trustmailer-node
npm login
npm publish --access public
```

Complete npm's browser login/2FA prompts. Omit `npm login` if already signed in
with the correct account. Login uses npm's default registry unless your npm
configuration overrides it. Do not add `--provenance` to this local publish; CI
trusted publishing supplies provenance automatically.

To build from a fresh checkout, run in `crates/trustmailer-node`:

```sh
npm ci
npm run build -- -- --locked
npm test
```

This requires Node.js, a Rust toolchain, native compiler tools, and Python 3 for
the MIME parser regression tests. `npm publish --dry-run` previews the files
without publishing or requiring a separate tarball.

## Configure npm trusted publishing after the first publish

Open <https://www.npmjs.com/package/@povio/trustmailer/access>, then find
**Trusted publishing** in the package settings and add **GitHub Actions**:

| Field | Value |
| --- | --- |
| Organization or user | `povio` |
| Repository | `trustmailer` |
| Workflow filename | `napi-publish.yml` |
| Environment name | Leave blank |
| Allowed actions | Enable direct `npm publish` |

Only this one package needs configuration. No `NPM_TOKEN` GitHub secret is needed.
The workflow uses Node 24, npm 11.6.2, and `id-token: write`. Its publish job runs
on a GitHub-hosted runner. The package's repository URL points to this fork.

New trusted publisher configurations must complete a successful CI publish
within two days; otherwise recreate the expired configuration. Create the
configuration when the next release is ready.

Reference: <https://docs.npmjs.com/trusted-publishers/>

## Next release: all platforms from GitHub Actions

Commit and push the prepared package and workflow changes, and merge them into
the release branch before tagging. The workflow builds eight binaries and
bundles all of them into the single main package, increasing its download size
in exchange for one package and one trusted publisher.

After the local `0.1.1` publish, use a new version such as `0.1.2`; npm versions
cannot be overwritten. From `crates/trustmailer-node`:

```sh
npm version patch --no-git-tag-version
npm run build -- -- --locked
npm test
```

Commit and push the version/lockfile and regenerated binding changes. Then,
from the repository root at that committed release:

```sh
git tag v0.1.2
git push origin v0.1.2
```

Pushing the tag publishes the release after the matrix builds and tests pass.
Cross-compiled Linux binaries are also loaded and tested in Debian Bookworm
(Node.js and Bun, glibc) and Alpine (Node.js, musl) containers on x64 and ARM64
runners; publishing waits for those jobs.
The tag must match `v` plus the package version. Manual workflow dispatch builds
and tests artifacts only; it does not publish from a branch.

The trusted publisher cannot be verified end to end until the package exists,
the npm settings are saved, these changes are on GitHub, and a new release tag
is pushed.
