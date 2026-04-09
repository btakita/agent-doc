---
agent_doc_session: a9421282-fd96-4943-9af5-3561ed5cb799
agent_doc_format: template
agent_doc_write: crdt
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
❯ I have a dedicated Alma Linux server on liquidweb.com
❯ I want to run a wordpress site using a docker container.
❯ I want to configure a good setup with docker containers.
Should we use kubernetes or docker swarm? I don't have experience to kubernetes but I can learn. What do you suggest. 
### Re: Kubernetes vs Docker Swarm for single-server WordPress (HEAD)

**Use Docker Compose — not Kubernetes or Swarm.**

For a single dedicated server running one WordPress site, neither Kubernetes nor Docker Swarm adds value. They're orchestrators designed for multi-node clusters. On one machine, they add complexity with no real benefit.

Docker Compose is the right tool:
- Purpose-built for single-host multi-container apps
- Excellent WordPress + Nginx + MariaDB examples everywhere
- Simple to operate, easy to update, trivial to back up

**Typical stack:**

```
Nginx (reverse proxy + SSL termination)
  ↕
WordPress (php-fpm container)
  ↕
MariaDB (database container)
  ↕
Redis (optional, object cache)
```

Certbot or Traefik handles Let's Encrypt SSL. Traefik integrates directly with Docker Compose and auto-renews certs — less manual work than Certbot.

**When to reconsider:**
- k3s (lightweight K8s): if you add more servers later and need automated failover
- Docker Swarm: basically never — k3s has replaced it and Swarm is in maintenance mode

Want me to draft a `docker-compose.yml` + Nginx config for this setup?
<!-- agent:boundary:f503d2b1 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [ ] Draft docker-compose.yml + Nginx config (pending user confirmation)
<!-- /agent:pending -->
