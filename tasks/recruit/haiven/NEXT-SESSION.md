# Next-session prompt — Haiven WebSocket Hub

Paste everything in the fenced block below into a fresh session.

```
agent-doc /home/brian/work/btakita/agent-loop/tasks/recruit/haiven.md

Context: continuing the Haiven WebSocket Hub take-home. Code lives in the submodule
src/haiven-websocket-hub (its own git remote; everything through 6925c77 is committed
AND pushed, superproject gitlinks bumped, working tree clean). The deliverable is
tasks/recruit/haiven/haiven-websocket-hub-takehome-v2.md — Brian is hand-writing it,
so it is HIS voice: correct facts, do not rewrite prose. Assignment:
tasks/recruit/haiven/backend-coding-challenge.md.

=== FIRST: the session doc may be wedged ===
The live editor buffer had a duplicate closing marker (6 opens / 7 closes) while disk
was clean and balanced. That blocks preflight AND session-check at the integrity gate,
and it does not self-heal — force-disk and `reset --from-current` both refuse because
they parse the buffer first. An editor restart cleared it twice. If preflight errors
with "malformed agent-doc component tree", restart the IDE, then re-run.
See memory: reference_agent_doc_recovery_replay_duplicate_boundary_wedges_integrity_gate

=== TOP PRIORITY: a benchmark result that cannot be true ===
dspool reports SOCKETS WITH LOWER p50 THAN IN-PROCESS. That is impossible — a socket
strictly adds marshal + syscall + transit + parse on top of the in-process path. If
sockets measure faster, the measurement is wrong. Do not explain it away; find the bug.

Chart (5k clients, 50k msgs, pool/streams=16, batch=64, room-size 10, 0 shed, 50k samples):
  in-process  p50 44.42ms  p90 66.24ms  5.24 M deliveries/s
  ws   b=64   p50  2.45ms  p90 29.74ms  2.96 M deliveries/s
  grpc b=64   p50  2.62ms  p90 28.62ms  3.34 M deliveries/s

Partial theory that FITS in-process but NOT the legs: in-process Handle only enqueues
(~100ns) while ws/grpc Handle also marshals (~1.5us), so in-process accepts ~15x faster,
its queue goes deeper, and every message waits behind the backlog (Little's law, L=λW).
In-process drains in ~95ms and its median waits ~44ms — consistent. But ws drains SLOWER
(~169ms), so its median should be ~84ms, NOT 2.45ms. So the leg's latency sampling is
probably broken.

Suspects, in order:
  - latencyRec in cmd/loadtest/dspool.go (send/observe/percentiles)
  - the Observer hook in downstream/{wspool,grpcpool} route()
  - the corr round-trip through EchoToTopic + the JSON-array batch encode/decode
Sanity check to run first: assert sum(latency samples) reconciles against drain rate,
and that every corr is stamped once and observed once.
DO NOT publish that chart until it reconciles.

=== Numbers that are TRUSTED (measured, reconciled) ===
- Hub: 100M conns in ~106 GiB (measured, not projected); ~740 B heap/conn;
  3.59 M deliveries/s at 1M clients; breaker sheds 93% under 10x overload with all
  1M conns live and heap bounded.
- Sharding (`-shards 1` = unsharded, reproducible from the shipped binary):
  fan-out @1M 1.41 -> 3.59 M/s; unsharded is FLAT with scale (single-bottleneck
  signature); connect 2.211s -> 0.895s; heap/conn identical (738 B) — speed cost no memory.
- friend-presence is unbounded (~76 B/friendship; 38x per-conn RSS at median 3000).
  `-friends-frame 50` flattens it: median 300 -> 3000 (10x more friends) = +0.4% RSS.
- Leg latency at MATCHED conditions, 0 shed, honest queued control:
    at rest   inproc 14us  / ws 521us / grpc 223us
    50k burst inproc 49ms  / ws 79ms  / grpc 59ms
  Boundary costs ~16-37x at rest, only ~1.2-1.6x under burst (queueing dominates both).
- gRPC beats ws on throughput AND latency once HTTP/2 windows are tuned. Its DEFAULTS
  are a trap: stock 64 KiB windows shed 71.5% where ws shed 0%.
- Opportunistic batching: halves the pool needed (pool=2+batch = 0 shed vs pool=4
  unbatched), cuts syscalls ~6.5x even when not needed, and LOWERS p50 35-43% under
  backlog. avg batch self-tunes (15.0 -> 6.5 as the pool grows) = a pressure gauge.

=== Numbers that are SUPERSEDED — do not reuse, they are in the deliverable ===
  "~21k msg/s leg"          -> settle() charged its own 2s quiet window. Real ~316-332k/s.
  "in-process 3-4us"        -> control did work inline, violating §4.2. Real 14us.
  "boundary costs ~15,000x" -> same inline-control artifact. Real ~16-37x / ~1.2-1.6x.
  "RTT ~50us"               -> real ~220-520us.

=== Fix the deliverable (v2 carries the bad numbers above) ===
  line ~29  "~21k messages/sec" -> ~316k/s (ws) / ~332k/s (grpc), pooled+batched.
  line ~30  "332.4k sends/sec" is attributed to gRPC WITHOUT batching; that figure is
            WITH batch=64 at pool=16. Unbatched pool=16 is 291.5k.
  line ~30  "Opportunistic batching would achieve millions per second" is NOT measured
            (measured 332k msg/s = 3.34 M deliveries/s). Sentence also truncates
            mid-clause: "...but at an increased latency due to ."
  line ~74  "<100GB RAM" -> ~106 GiB measured.
  line ~86  "pooling ... due to dropped sockets under load" — never observed; the actual
            finding was the breaker shedding. Also "upstream" -> "downstream", and the
            pool was demonstrated, not adopted.
  line ~20  "DropNewest: ..." is unfinished.
  structure two "# 1" headers; "### 2.1" sits under "# 1 High-level Design".

=== v2 is missing assignment-required sections that ALREADY EXIST in DESIGN.md ===
  stateful vs stateless (§3) · backpressure (§4.3) · fan-out (§4.4) · heartbeat (§4.5) ·
  edge cases: reconnect/dup, out-of-order, oversized, downstream down, slow consumers (§6) ·
  metrics + how we know it works (§11) · what to defer (§12) · questions for backend and
  frontend teams + how to split the work across 3 engineers (§10) · envelope schema (§8) ·
  single message walkthrough (Appendix).
Brian also asked for a SHORT DESIGN-DECISION LOG from our sessions — not written yet.
Good candidates: gorilla -> gnet+gobwas; actor registry -> sharded locks (measured);
typing DropNewest -> ConflateByEntity + TTL; friend graph -> friend-frame; ws pool ->
gRPC streams; "route by volume" withdrawn in favour of failure-mode + resource-profile.

=== Tension worth raising with Brian ===
The brief says "limit the use of AI" and "spend no more than 2-4 hours", and judges
reasoning over artifacts. The prototype is far past that. v2 §1 is transparent about
Claude Code use (right call), but consider framing the prototype as VALIDATION of his
reasoning and leading with the reasoning itself. The take-home review is the highest-value
remaining work — the benchmarking has gone well beyond what is asked.

=== Method note (three measurement bugs this session, ALL flattering in-process) ===
Measure the control, not just the subject. Hold buffering equal across arms. Never vary
shedding and latency together — a shed message contributes no latency sample, so a
shedding arm reads artificially fast.
```
