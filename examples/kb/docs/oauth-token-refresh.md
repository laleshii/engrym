---
id: oauth-token-refresh
title: OAuth token refresh flow
altitude: 3
topics:
  - backend/auth/oauth
  - security/tokens
relations:
  - { type: refines,    target: auth-architecture }
  - { type: depends_on, target: token-store }
summary: How expired access tokens are exchanged for new ones.
---
# OAuth token refresh flow

When an access token expires, the client posts its refresh token to the
`/oauth/token` endpoint with `grant_type=refresh_token`. The server validates
the refresh token against [[token-store]], rotates it, and returns a new
access/refresh pair.

## Rotation

Refresh tokens are single-use. Replay of a consumed refresh token revokes the
whole token family — a stolen token cannot outlive one use.
