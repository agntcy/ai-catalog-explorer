# Security Policy

## Reporting A Vulnerability

Please do not report security vulnerabilities through public GitHub issues,
pull requests, or discussions.

Use GitHub's private vulnerability reporting flow for this repository:

- <https://github.com/agntcy/ai-catalog-explorer/security/advisories/new>

If private reporting is not available to you, contact the project maintainers
privately through GitHub and request a secure channel before sharing sensitive
details.

When reporting a vulnerability, please include:

- the affected component or workflow
- reproduction steps or a proof of concept
- the expected security impact
- any known mitigations or suggested fixes

## Supported Versions

Security fixes are provided on a best-effort basis for the default branch.

| Branch or Version | Supported |
| --- | --- |
| `main` | Yes |
| earlier versions and feature branches | No |

## Scope

Security reports are especially relevant for:

- the server-side URL fetcher (private addresses, redirects, DNS rebinding, size and time limits)
- the index file and the inspect endpoints
- GitHub workflows and dependency automation
- any repository content that could affect integrity, provenance, or disclosure

## Disclosure Process

The maintainers will try to:

- acknowledge receipt promptly
- validate and triage the report
- coordinate remediation and disclosure timing when appropriate

Please avoid public disclosure until a fix or mitigation is available.
