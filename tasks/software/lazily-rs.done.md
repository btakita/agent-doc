# Agent Doc Completed Work

- 2026-06-01 [#perf-bench] #performance Add a Criterion benchmark suite for Context and ThreadSafeContext covering cached reads, cold first get, dependency fan-out, memo equality suppression, effect flushing, batch storms, and threaded contention at 1/2/4/8/16 workers
- 2026-06-01 [#perf-profile] #performance Add lightweight benchmark instrumentation for recompute counts, duplicate speculative computes, dependency edge churn, effect queue depth, allocations, and ThreadSafeContext lock wait/hold time
- 2026-06-01 [#perf-layout] #performance Use benchmark results to optimize single-threaded Context storage and edge maintenance, starting with node storage locality and avoiding unnecessary HashSet/HashMap churn without changing public semantics
- 2026-06-01 [#perf-thread-dedup] #performance Prototype ThreadSafeContext in-flight recompute deduplication so concurrent first get callers share one computation when benchmarks show duplicate work is material
- 2026-06-01 [#perf-lock-eval] #performance Evaluate RwLock, sharded locks, or targeted CAS only after baseline data identifies lock contention, and require Loom/Shuttle safety models before adopting concurrency changes
