---
id: auth-architecture
title: Authentication architecture
altitude: 0
topics:
  - backend/auth
summary: How the service authenticates users and services end to end.
---
# Authentication architecture

The platform authenticates callers with short-lived access tokens issued after
an OAuth2 exchange. Sessions are kept alive by transparently refreshing expired
tokens, so users rarely re-authenticate. See [[oauth-token-refresh]] for the
refresh path and [[token-store]] for where credentials live.

## Trust boundaries

Service-to-service calls use the same token machinery with a machine identity,
so there is a single code path to audit.
