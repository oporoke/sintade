# ADR-0007: `tenancy` crate, a `WorkspaceDirectory` port in `identity`, and the role matrix

## Status

Accepted — 2026-09-25. Supersedes [ADR-0004](0004-identity-writes-tenancy-tables-until-day-18.md).

## Context

ADR-0004 let `identity` write `workspaces`/`memberships` directly until `tenancy` existed, and
said Day 18 must remove that exception. Two constraints shape how:

- `identity` and `tenancy` are both L2 (CLAUDE.md rule 2). The one allowed edge between them
  is `tenancy → identity` (`docs/design.md`'s layering table), so `identity` cannot call
  `tenancy`.
- Registration must create the user and their personal workspace in one transaction (US-01),
  and login/refresh need the user's workspace to issue the access cookie.

`docs/design.md` also names `Role` and `Permission` as shared-kernel types ("Tenancy | all |
Shared kernel | `WorkspaceId`, `Role`, `Permission`") and lists the four roles
owner/admin/member/viewer, but gives no permission set. README §5 has a draft matrix, itself
marked `TODO: Verify`.

## Decision

1. **`crates/tenancy`** owns `workspaces` and `memberships`. `TenancyService` provides
   `create_personal_workspace` and `personal_workspace_for` (both run on the caller's
   `&mut PgConnection` so they can join another module's transaction), `list_memberships`, and
   `authorize(user, Permission, WorkspaceId) -> Result<Role, AuthorizeError>`.
2. **`identity` defines a port, `WorkspaceDirectory`**, with just the two workspace operations
   it needs. `TenancyService` implements it over the allowed `tenancy → identity` edge, and
   `bin/api` injects it into `IdentityService`. `identity` no longer touches tenancy tables.
3. **`GET /me` is composed in the handler**: the user from `identity`, memberships from
   `tenancy`. The JSON shape is unchanged.
4. **`Role` and `Permission` live in `kernel`**; the matrix (`tenancy::grants`) lives in
   `tenancy`, which owns roles. It follows README §5's draft and its hierarchy
   (`owner` ⊃ `admin` ⊃ `member` ⊃ `viewer`):

   | Permission | owner | admin | member | viewer |
   | --- | --- | --- | --- | --- |
   | `ViewWorkspace` | ✓ | ✓ | ✓ | ✓ |
   | `CreateRecording` | ✓ | ✓ | ✓ | |
   | `EditRecording` | ✓ | ✓ | ✓ | |
   | `DeleteRecording` | ✓ | ✓ | ✓ | |
   | `ManageMembers` | ✓ | ✓ | | |
   | `ManageBilling` | ✓ | | | |

   The one choice beyond the draft is admin + billing: the draft says `TODO: Verify`, and this
   starts closed (owner only). The whole matrix stays `TODO: Verify` until V1's team features
   (invites, role changes) ship. The draft's "own vs others' recordings" split needs the
   resource owner as well as the role, so it is left for the catalog days
   (`ctx.require(Permission, &resource)`).
5. **`authorize` errors**: not a member (or no such workspace) → `404`; member without the
   permission → `403`. The role is read from `memberships` on every request, never trusted
   from the cookie.

## Consequences

- The migration that created `workspaces`/`memberships` still carries its ADR-0004 comment.
  Applied migrations are never edited, so that comment stays as history.
- `WorkspaceContext` and `Access::Workspace` have no production consumer until Day 33
  (`POST /recordings`). They carry `#[cfg_attr(not(test), expect(dead_code))]`, which turns
  into a compile error once a real consumer arrives, forcing its removal.
- Every route is declared in `bin/api/src/routes/mod.rs::table()` with an `Access` class. The
  tenant-isolation harness generates its cases from that table, and it fails on any
  `Workspace` route missing from its cross-tenant table.
