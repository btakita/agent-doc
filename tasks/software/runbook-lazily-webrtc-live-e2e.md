# Runbook: lazily WebRTC live two-host e2e (#h6qb)

**Status:** operator-gated. Agent-doable slices (#lzwebrtcwire localhost handshake, #lzstr0mframe SCTP backpressure, #lzstr0mpolldrive driver I/O errors, this runbook + `examples/webrtc_live_e2e.rs`) are complete. What remains is a live two-host / NAT round trip through the deployed `#yxjw` Cloudflare signaling Worker — that needs two real hosts and a deployed Worker URL, which only the operator can provide.

**Goal:** prove the networked `Str0mNet` backend (`src/str0m_net.rs`) carries a permission-filtered `IpcMessage::Snapshot` end-to-end across two real hosts on different networks, through the live signaling Worker, with no silent drops. On success this run closes review `#h6qb`.

## Prerequisites

1. **Deployed signaling Worker.** The Cloudflare Worker in `src/lazily-rs/signaling/` (`wrangler.toml`: `name = "lazily-signaling"`, `SIGNALING_MODE = "open"`). Deploy with `npx wrangler deploy` from `src/lazily-rs/signaling/`; capture the resulting `wss://…workers.dev` URL.
2. **Two hosts on different networks / NATs.** Host A (offerer) and Host B (answerer). At least one must be behind a consumer-grade NAT (the realistic deployment target). Both need outbound HTTPS/WSS to the Worker and outbound UDP to each other's public IP.
3. **Rust toolchain** on both hosts. `cargo --version` ≥ 1.85 (edition 2024).
4. **Firewall:** both hosts must allow **inbound UDP on the bind port** for the ICE host candidate to be reachable. The example defaults to `0.0.0.0:0` (OS-assigned ephemeral port); if running behind `ufw`/`iptables`, either pin a port via the 5th positional arg or allow the ephemeral range. Cloud providers' security groups need the same hole.
5. **Clone** of `btakita/agent-loop` (superproject) with submodules: `git clone --recurse-submodules …`.

## Build the driver (both hosts)

From `src/lazily-rs/`:

```sh
cargo build --release --example webrtc_live_e2e \
    --features "signaling-client webrtc-str0m"
```

The binary lands at `target/release/examples/webrtc_live_e2e`. Both hosts build independently; the binary is single-shot (one handshake, one snapshot, then exit).

## Execution

Pick a shared `session` id (any string) and distinct `peer_id`s (1 and 2). The 4th positional arg (`offer_to_peer_id`) selects **offerer** mode; its absence selects **answerer** mode.

### Host A — offerer (run first if NAT traversal is symmetric)

```sh
SIGNALING_URL=wss://lazily-signaling.<your-subdomain>.workers.dev
SESSION=live-e2e-$(date +%s)

./target/release/examples/webrtc_live_e2e \
    "$SIGNALING_URL" "$SESSION" 1 2
```

Args: `<signaling_url> <session> <peer_id=1> <offer_to_peer_id=2>`. Default `bind_addr=0.0.0.0:0`.

### Host B — answerer (run second)

```sh
./target/release/examples/webrtc_live_e2e \
    "$SIGNALING_URL" "$SESSION" 2
```

Args: `<signaling_url> <session> <peer_id=2>`. No 4th arg ⇒ answerer mode.

> **Ordering:** the answerer must join the session *before* the offerer fires `offer_to_peer`, otherwise the offer is dropped by the relay (no peer-2 in the roster yet). Safest sequence: start Host B first (it blocks on `answer_next_offer`), then start Host A. The `webrtc_signaling.rs` test does the same — see `tests/webrtc_signaling.rs:184-217`.

## Expected output

Both sides print a structured `[lazily-e2e]` log. A successful run looks like:

**Host A (offerer):**
```
[lazily-e2e] connecting to wss://lazily-signaling.X.workers.dev/session/live-e2e-… as peer 1 (bind=0.0.0.0:0)
[lazily-e2e] joined; roster=[]
[lazily-e2e] offerer: offering to peer 2
[lazily-e2e] data channel OPEN with peer 2 (local_candidate=candidate:…)
[lazily-e2e] offerer sent permission-filtered snapshot (node 1 readable, node 2 denied)
[lazily-e2e] SUCCESS: offerer side complete. Answerer should print SUCCESS too.
```

**Host B (answerer):**
```
[lazily-e2e] connecting to wss://lazily-signaling.X.workers.dev/session/live-e2e-… as peer 2 (bind=0.0.0.0:0)
[lazily-e2e] joined; roster=[PeerId(1)]
[lazily-e2e] answerer: waiting for any offer
[lazily-e2e] data channel OPEN with peer 1 (local_candidate=candidate:…)
[lazily-e2e] answerer received snapshot nodes=[1] (expected [1])
[lazily-e2e] SUCCESS: live round-trip + permission filter verified
```

**Timing budget.** `OPEN_TIMEOUT = 30 s` (data channel must open), `RECV_TIMEOUT = 30 s` (snapshot must arrive after open). On a healthy NAT pair the whole exchange finishes in 2–10 s. If both sides print `SUCCESS`, the live round trip is proven.

## Troubleshooting

### `offer_to_peer` / `answer_next_offer` returns `Timeout`

The data channel did not open within `OPEN_TIMEOUT`. Causes, in order of likelihood:

1. **Answerer not joined when offerer fired.** Start Host B first; check Host B printed `joined; roster=[PeerId(1)]` before Host A prints `offering to peer 2`.
2. **UDP blocked inbound.** The ICE host candidate needs inbound UDP. Confirm the firewall hole (Prerequisites §4). Pin the bind port and check with `nc -ul <port>` from a third host.
3. **Symmetric NAT.** A single host candidate (what `Str0mNet` advertises today — `Candidate::host`, no STUN/TURN) cannot traverse symmetric NAT. Symptom: the offerer and answerer exchange SDP + ICE candidate over signaling, both report `data channel OPEN` never fires. Remediation requires adding a STUN/TURN module to `Str0m_net` (out of scope for `#h6qb`; file a follow-up if your target deployment is symmetric-NAT-only).
4. **Signaling Worker down or `SIGNALING_MODE=allowlist`.** Hit `wss://…/session/test` from a browser dev console; if the WS handshake fails, redeploy the Worker or flip `SIGNALING_MODE=open` in `wrangler.toml`.

### `Str0mNetError::Closed` mid-handshake

Driver thread exited. Post-`#lzstr0mpolldrive` this surfaces real I/O errors instead of silently stalling — check the driver's send path:

- `ECONNREFUSED` from `send_to` → peer is unreachable (ICMP port-unreachable); peer may have crashed or its firewall is dropping SCTP traffic. Restart both sides.
- `ENETUNREACH` / `EHOSTUNREACH` → local route is down; check `ip route` / VPN.
- `EBADF` → socket closed prematurely; should not happen without `Closed` on the public handle — file an issue with the full log.

### Snapshot arrives but `nodes != [1]`

Permission filter leaked. This is a correctness bug in `Snapshot::filter_readable` or `WebRtcSink`, not a network issue. File with the exact `nodes=[…]` value — pre-`#lzstr0mframe` the silent-drop bug could also produce truncated snapshots, but post-fix the burst path is reliable so a leaked node is a real filter bug.

### Burst / backpressure failures (advanced — not in the default run)

The default run sends one snapshot. To exercise the SCTP backpressure path that `#lzstr0mframe` fixed, modify the example to burst N × 8 KiB frames (mirror `tests/str0m_net.rs::burst_of_frames_arrives_in_order_under_backpressure`). If frames drop silently under burst on a real network, the fix regressed — file with the burst log.

## Proof capture (for `#h6qb` closure)

Save the following on success:

1. **Both hosts' full stderr** (`[lazily-e2e] …` lines, including `local_candidate=` strings).
2. **The signaling Worker URL + session id** used.
3. **Network topology summary:** host A OS + network type (home NAT / cloud / VPN), host B the same, and whether A→B / B→A UDP was direct or relayed (if known).
4. **The commit SHAs** of `lazily-rs` + superproject the binary was built from (`git rev-parse HEAD` in `src/lazily-rs/` and the repo root).

Append these to `tasks/software/lazily-rs.done.md` under a new `#h6qb` entry and `--review-resolve #h6qb`. Until that lands the review item stays `[operator-drive]` gated.

## References

- `examples/webrtc_live_e2e.rs` — the driver binary this runbook describes.
- `tests/webrtc_signaling.rs` — the localhost template (loopback signaling relay + two in-process peers).
- `tests/str0m_net.rs` — the networked-backend test (two real UDP sockets on `127.0.0.1`).
- `src/webrtc_signaling.rs` — `offer_to_peer` / `answer_next_offer` API surface.
- `src/str0m_net.rs` — `Str0mNet` networked backend (`#lzwebrtcnet`), including the `#lzstr0mframe` backpressure contract and `#lzstr0mpolldrive` I/O error handling.
- `src/lazily-rs/signaling/` — the Cloudflare Worker source (`wrangler.toml`, Durable Objects `SignalingRoom`).
