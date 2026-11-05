//! Bounded, content-keyed memo for pure document parsers (`#preflightqueuescale`).
//!
//! One preflight or closeout pass derives queue, claim, blocker, and
//! continuation projections from the same document text dozens of times, and
//! each derivation re-ran the whole-document parsers. On a 684 KB session
//! document that made `queue_maintenance` and the commit-finalize queue phase
//! cost tens of seconds: the work grew with (number of derivations) x
//! (document size) instead of with the queue components that changed.
//!
//! A [`ContentMemo`] is keyed by the exact input bytes, so it is only correct
//! for a function of that text alone (no file, clock, environment, or ambient
//! input). A changed input is a different key, so an entry can never be stale
//! and there is no invalidation to forget. Entries are evicted
//! least-recently-used under an entry and byte budget. Memos are meant to live
//! in a `thread_local!`, so concurrent tests and threads never share one; the
//! budgets bound what each thread retains (the element memo keeps at most four
//! documents, 8 MiB of keys; the queue-body memo 2 MiB).
//!
//! [`ContentMemo::evaluations`] counts real evaluations (misses and inputs
//! outside the memoized size band).

use std::collections::VecDeque;
use std::rc::Rc;

/// LRU memo of `f(text) -> T` keyed by the exact `text`.
pub struct ContentMemo<T> {
    entries: VecDeque<(Rc<str>, Rc<T>)>,
    key_bytes: usize,
    evaluations: u64,
    min_bytes: usize,
    max_entries: usize,
    max_key_bytes: usize,
}

impl<T: Clone> ContentMemo<T> {
    /// Memoize inputs of at least `min_bytes` (smaller ones are cheaper to
    /// parse than to look up), keeping at most `max_entries` entries whose keys
    /// total at most `max_key_bytes`.
    pub const fn new(min_bytes: usize, max_entries: usize, max_key_bytes: usize) -> Self {
        Self {
            entries: VecDeque::new(),
            key_bytes: 0,
            evaluations: 0,
            min_bytes,
            max_entries,
            max_key_bytes,
        }
    }

    /// Real evaluations of the memoized function so far.
    pub fn evaluations(&self) -> u64 {
        self.evaluations
    }

    /// The memoized value of `eval(text)`. An `Err` is returned as is and never
    /// memoized, so a failing input is re-evaluated on every call.
    pub fn get_or_try_insert<E>(
        &mut self,
        text: &str,
        eval: impl FnOnce(&str) -> Result<T, E>,
    ) -> Result<T, E> {
        match self.lookup(text) {
            Lookup::Hit(value) => Ok(value),
            lookup => {
                let value = eval(text)?;
                self.record(text, &value, lookup);
                Ok(value)
            }
        }
    }

    fn lookup(&mut self, text: &str) -> Lookup<T> {
        if text.len() < self.min_bytes || text.len() > self.max_key_bytes {
            self.evaluations += 1;
            return Lookup::Unmemoized;
        }
        if let Some(index) = self
            .entries
            .iter()
            .position(|(key, _)| key.len() == text.len() && **key == *text)
            && let Some(entry) = self.entries.remove(index)
        {
            let value = (*entry.1).clone();
            self.entries.push_front(entry);
            return Lookup::Hit(value);
        }
        self.evaluations += 1;
        Lookup::Miss
    }

    fn record(&mut self, text: &str, value: &T, lookup: Lookup<T>) {
        if !matches!(lookup, Lookup::Miss) {
            return;
        }
        self.key_bytes += text.len();
        self.entries
            .push_front((Rc::from(text), Rc::new(value.clone())));
        while self.entries.len() > self.max_entries || self.key_bytes > self.max_key_bytes {
            let Some((key, _)) = self.entries.pop_back() else {
                break;
            };
            self.key_bytes -= key.len();
        }
    }
}

enum Lookup<T> {
    Hit(T),
    Miss,
    Unmemoized,
}

/// [`ContentMemo::get_or_try_insert`] on a thread-local memo, WITHOUT holding
/// its borrow while `eval` runs, so a parser that recursively calls its own
/// memoized entry point (a nested body) cannot hit a `RefCell` double borrow.
pub fn memoized<T: Clone + 'static, E>(
    memo: &'static std::thread::LocalKey<std::cell::RefCell<ContentMemo<T>>>,
    text: &str,
    eval: impl FnOnce(&str) -> Result<T, E>,
) -> Result<T, E> {
    let lookup = memo.with(|memo| memo.borrow_mut().lookup(text));
    if let Lookup::Hit(value) = lookup {
        return Ok(value);
    }
    let value = eval(text)?;
    memo.with(|memo| memo.borrow_mut().record(text, &value, lookup));
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memo_evaluates_each_distinct_text_once_and_never_memoizes_errors() {
        let mut memo: ContentMemo<usize> = ContentMemo::new(4, 2, 1024);
        let ok = |text: &str| Ok::<_, ()>(text.len());
        assert_eq!(memo.get_or_try_insert("aaaa", ok), Ok(4));
        assert_eq!(memo.get_or_try_insert("aaaa", ok), Ok(4));
        assert_eq!(memo.evaluations(), 1, "a repeated text is a hit");
        assert_eq!(memo.get_or_try_insert("bbbbb", ok), Ok(5));
        assert_eq!(memo.evaluations(), 2);
        // Small inputs are evaluated every time.
        memo.get_or_try_insert("cc", ok).unwrap();
        memo.get_or_try_insert("cc", ok).unwrap();
        assert_eq!(memo.evaluations(), 4);
        // Errors are returned and re-evaluated, never cached.
        let err = |_: &str| Err::<usize, _>("bad");
        assert_eq!(memo.get_or_try_insert("dddd", err), Err("bad"));
        assert_eq!(memo.get_or_try_insert("dddd", err), Err("bad"));
        assert_eq!(memo.evaluations(), 6);
    }

    thread_local! {
        static NESTED: std::cell::RefCell<ContentMemo<usize>> =
            const { std::cell::RefCell::new(ContentMemo::new(1, 4, 1024)) };
    }

    fn nested_len(text: &str) -> Result<usize, ()> {
        memoized(&NESTED, text, |text| match text.split_once('/') {
            Some((_, rest)) => nested_len(rest).map(|inner| inner + 1),
            None => Ok(0),
        })
    }

    #[test]
    fn memoized_allows_a_parser_to_recurse_into_its_own_memo() {
        assert_eq!(nested_len("a/b/c"), Ok(2));
        assert_eq!(nested_len("a/b/c"), Ok(2));
        assert_eq!(NESTED.with(|memo| memo.borrow().evaluations()), 3);
    }

    #[test]
    fn memo_evicts_least_recently_used_under_entry_and_byte_budgets() {
        let mut memo: ContentMemo<usize> = ContentMemo::new(1, 2, 10);
        let ok = |text: &str| Ok::<_, ()>(text.len());
        memo.get_or_try_insert("aaaa", ok).unwrap();
        memo.get_or_try_insert("bbbb", ok).unwrap();
        memo.get_or_try_insert("aaaa", ok).unwrap(); // refresh `aaaa`
        memo.get_or_try_insert("cccc", ok).unwrap(); // evicts `bbbb`
        assert_eq!(memo.evaluations(), 3);
        memo.get_or_try_insert("aaaa", ok).unwrap();
        assert_eq!(memo.evaluations(), 3, "the refreshed entry survived");
        memo.get_or_try_insert("bbbb", ok).unwrap();
        assert_eq!(
            memo.evaluations(),
            4,
            "the least recently used entry was evicted"
        );
        // An input above the byte budget is evaluated but never stored.
        memo.get_or_try_insert("eeeeeeeeeeee", ok).unwrap();
        memo.get_or_try_insert("eeeeeeeeeeee", ok).unwrap();
        assert_eq!(memo.evaluations(), 6);
    }
}
