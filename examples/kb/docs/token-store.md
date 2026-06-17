---
id: token-store
title: Token store
altitude: 2
topics:
  - backend/auth/oauth
  - security/tokens
relations:
  - { type: part_of, target: auth-architecture }
summary: Where refresh tokens are persisted and how they are rotated.
---
# Token store

Refresh tokens are persisted as salted hashes in a dedicated table keyed by
token family. The store exposes `validate`, `rotate`, and `revoke_family`
operations used by the [[oauth-token-refresh]] flow.

## Persistence

Each row records the family id, the current hash, and an issued-at timestamp so
expired families can be reaped.
