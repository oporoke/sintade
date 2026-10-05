# Submitting the extension (Chrome Web Store and Microsoft Edge Add-ons)

**Status (Day 66): prepared, not submitted.** The package, listing text, images, permission
justifications and privacy-policy page are ready. Submitting needs two things only the product
owner can supply: developer accounts and the production address.

## What the owner has to provide

1. **The production origin** the extension talks to (the Sintade web/API address, HTTPS, one host).
   Not decided yet (**TODO: Verify**); it is baked in at build time as the single host permission.
2. **A Chrome Web Store developer account** — one-time US$5 registration fee,
   <https://chrome.google.com/webstore/devconsole>, with a verified contact email (and 2-step
   verification on the Google account).
3. **A Microsoft Partner Center account** with the Edge Add-ons program — free,
   <https://partner.microsoft.com/dashboard/microsoftedge>.
4. **Company details and support contact** to complete the privacy policy's bracketed items, and a
   legal review of that policy (Tanzanian advocate, `docs/design.md` §19).
5. A **reviewer test account** (or approval to describe signing up) for the review notes.

## Steps

```bash
# 1. Build the store package for the production address
cd extension
SINTADE_ORIGIN=https://<production origin> npm run package
#    -> extension/artifacts/sintade-extension-<version>.zip
#    It fails if the manifest isn't store-fit (extra permissions, a development key or host, ...).

# 2. Regenerate the screenshots against production (see docs/store/listing.md), then review them.

# 3. Publish the privacy policy: deploy the web app so https://<production origin>/privacy is live.
```

4. **Chrome Web Store:** Developer Dashboard → *Add new item* → upload the zip → *Store listing*
   (copy from `docs/store/listing.md`, upload `docs/store/assets/`) → *Privacy practices*
   (single purpose, permission justifications, data disclosures, privacy-policy URL) →
   *Distribution* (public, all regions or Tanzania + chosen regions) → *Submit for review*.
5. **Microsoft Edge Add-ons:** Partner Center → Edge → *Create new extension* → upload the same zip
   → *Properties* (category, privacy-policy URL) → *Store listings* → *Notes for certification* →
   *Publish*.
6. Reviews usually take from a day to a few weeks; both stores email the contact address.
   A rejection names the policy; fix, bump the version in `extension/package.json`, and resubmit.

## What the reviewers will check, and where we stand

| Check | Status |
| --- | --- |
| Single, clear purpose | Stated in the listing and policy |
| Minimal permissions, each justified | 5 permissions + 1 host (`docs/store/listing.md`); the package builder fails on any other |
| No remote code, no obfuscation | Everything bundled; minified only (no obfuscation) |
| Privacy policy at a public URL, accurate to the product | Page written; needs the production URL and legal review |
| Data use disclosures match the code | See `docs/store/listing.md`; kept in step by the policy-page test and the manifest check |
| Works without surprises | Packaged zip loads in Chrome and Edge in CI (`extension/e2e/package.spec.ts`) |
| Screenshots show the real product | Generated from the real extension; regenerate against production |

## Updating later

Bump `version` in `extension/package.json`, run `npm run package`, upload the new zip in each
store's dashboard. The extension's logic lives in the web repo, so a change to `capture/` needs a
new store version too.
