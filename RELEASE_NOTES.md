# Taoki v1.3.2 Release Notes

## Highlights

Install scripts are now served from GitHub Releases instead of `raw.githubusercontent.com`, eliminating CDN caching issues that caused stale install scripts on updates.

## Changes

### Install URLs use GitHub Releases

- Install scripts (`install.sh`, `install.ps1`, `uninstall.sh`) are now published as standalone GitHub Release assets alongside the platform binaries.
- The install URL is now `https://github.com/naejin/taoki/releases/latest/download/install.sh` -- a 302 redirect that always resolves to the latest release's asset. No more stale CDN cache.
- All documented install commands updated across README, CLAUDE.md, RELEASE_NOTES, and the scripts' own fallback messages.
- The release pipeline (`release.yml`) now checks out the `scripts/` directory and copies the install scripts into the release artifacts.

**Previous behavior:** `raw.githubusercontent.com` aggressively caches files via CDN, so re-running the install command could serve a stale script for minutes after a release.

**New behavior:** `/releases/latest/download/` is a redirect, not a static file. The redirect target is versioned and immutable -- every release gets the exact script that shipped with it.

## Upgrading

```bash
curl -fsSL https://github.com/naejin/taoki/releases/latest/download/install.sh -o /tmp/taoki-install.sh && bash /tmp/taoki-install.sh
```

Or if installed via marketplace: `claude plugin install taoki@monet-plugins`

No cache or protocol changes -- fully compatible with v1.3.1.
