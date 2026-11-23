//! FIFO order for one supervisor's actor-lifecycle publications (GH #438).
//!
//! Every lifecycle transition the supervisor publishes to the project
//! controller takes a ticket at the point the transition happens, then
//! publishes only when every earlier ticket has finished. A publication that
//! an IPC handler defers until after its receipt therefore still reaches the
//! controller before any transition the supervisor makes later, such as the
//! child-exit or prompt-ready transition that follows a Stop Agent or Clear.

use parking_lot::{Condvar, Mutex};

#[derive(Debug, Default)]
struct Cursor {
    /// Next ticket to hand out.
    next: u64,
    /// Ticket whose publication may run now.
    serving: u64,
}

#[derive(Debug, Default)]
pub(crate) struct LifecyclePublicationOrder {
    cursor: Mutex<Cursor>,
    turn: Condvar,
}

impl LifecyclePublicationOrder {
    /// Take the next position in publication order. The caller must pass the
    /// ticket to [`Self::wait_turn`] exactly once, or every later publication
    /// waits forever.
    pub(crate) fn reserve(&self) -> u64 {
        let mut cursor = self.cursor.lock();
        let ticket = cursor.next;
        cursor.next += 1;
        ticket
    }

    /// True while any reserved publication has not finished.
    pub(crate) fn pending(&self) -> bool {
        let cursor = self.cursor.lock();
        cursor.next != cursor.serving
    }

    /// Block until every earlier ticket has finished. The returned turn ends,
    /// and admits the next ticket, when it is dropped.
    pub(crate) fn wait_turn(&self, ticket: u64) -> LifecyclePublicationTurn<'_> {
        let mut cursor = self.cursor.lock();
        while cursor.serving != ticket {
            self.turn.wait(&mut cursor);
        }
        LifecyclePublicationTurn { order: self }
    }
}

pub(crate) struct LifecyclePublicationTurn<'a> {
    order: &'a LifecyclePublicationOrder,
}

impl Drop for LifecyclePublicationTurn<'_> {
    fn drop(&mut self) {
        let mut cursor = self.order.cursor.lock();
        cursor.serving += 1;
        self.order.turn.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::time::Duration;

    #[test]
    fn publications_run_in_reservation_order_whatever_thread_waits_first() {
        let order = Arc::new(LifecyclePublicationOrder::default());
        let published = Arc::new(Mutex::new(Vec::new()));
        let first = order.reserve();
        let second = order.reserve();
        assert!(order.pending());

        // The later ticket starts waiting first; it must still publish second.
        let waiter = {
            let (order, published) = (order.clone(), published.clone());
            std::thread::spawn(move || {
                let _turn = order.wait_turn(second);
                published.lock().push(second);
            })
        };
        std::thread::sleep(Duration::from_millis(50));
        assert!(published.lock().is_empty(), "ticket {second} ran early");

        {
            let _turn = order.wait_turn(first);
            published.lock().push(first);
        }
        waiter.join().unwrap();

        assert_eq!(*published.lock(), vec![first, second]);
        assert!(!order.pending());
    }
}
