# Contributing

Contributions are accepted through pull requests.

## Workflow

1. Create a feature branch from `main`.
2. Make focused changes with tests and documentation updates when relevant.
3. Run the local checks before opening a pull request.
4. Sign off every commit (`git commit -s`) to certify the
   [Developer Certificate of Origin](https://developercertificate.org/).
5. Open a pull request for review; do not push directly to `main`. The title
   must follow [Conventional Commits](https://www.conventionalcommits.org/),
   for example `feat: add a filter to the index`.

## Local Checks

Run the baseline checks before requesting review:

```sh
just build
just lint
just test
```

## Change Expectations

- Keep changes scoped to a single concern when possible.
- Add or update tests for behavior changes.
- Update `README.md` when the user-facing workflow changes.
- Preserve the repository license headers on source and script files.
- Keep dependency lockfile updates in their own pull request.

## Review and Merge

- Pull requests should describe the motivation, implementation approach, and validation performed.
- Changes land through reviewed pull requests.
