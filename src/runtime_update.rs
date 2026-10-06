//! Shared runtime-recycle boundary for binary, native-library, and editor-plugin updates.
//!
//! An update is not complete while a long-lived supervisor or controller can
//! continue serving the generation that owned the replaced package.  Keep the
//! ordering here: supervisors first persist their turn-boundary handoff, then
//! controllers are asked to recycle at their safe idle boundary.

pub(crate) fn recycle_on_update_enabled() -> bool {
    match std::env::var("AGENT_DOC_RECYCLE_ON_INSTALL") {
        Ok(value) => !matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "0" | "false" | "no" | "off"
        ),
        Err(_) => true,
    }
}

/// Finish an update by recycling every extant supervisor and controller.
///
/// This deliberately uses the generic all-controller fan-out rather than the
/// install-optimized fan-out: an editor-plugin update can leave a stale
/// endpoint in an otherwise idle project, and its controller may run the same
/// binary while still owning state derived from the replaced plugin generation.
/// Both handoffs are non-forcing and therefore wait for their safe idle/turn
/// boundaries.
pub(crate) fn recycle_existing_runtimes_after_update(surface: &str) {
    if !recycle_on_update_enabled() {
        eprintln!(
            "[{surface}] note: automatic runtime recycle opted out (AGENT_DOC_RECYCLE_ON_INSTALL falsey)"
        );
        return;
    }

    let (supervisors, controllers) = recycle_fleet_with(
        || {
            agent_doc_controller_io::project_controller::recycle_supervisors_all_projects_force(
                false,
            )
        },
        || {
            agent_doc_controller_io::project_controller::recycle_controllers_all_projects_force(
                false,
            )
        },
    );
    match supervisors {
        Ok((marked, skipped)) => eprintln!(
            "[{surface}] runtime recycle: {marked} supervisor(s) marked for the next idle boundary, {skipped} skipped"
        ),
        Err(error) => eprintln!(
            "[{surface}] warning: supervisor recycle fan-out failed ({error:#}); existing supervisors may still serve the previous generation"
        ),
    }

    match controllers {
        Ok((marked, skipped)) => eprintln!(
            "[{surface}] runtime recycle: {marked} controller(s) marked after supervisor handoff, {skipped} skipped"
        ),
        Err(error) => eprintln!(
            "[{surface}] warning: controller recycle fan-out failed ({error:#}); existing controllers may still serve the previous generation"
        ),
    }
}

fn recycle_fleet_with(
    recycle_supervisors: impl FnOnce() -> anyhow::Result<(usize, usize)>,
    recycle_controllers: impl FnOnce() -> anyhow::Result<(usize, usize)>,
) -> (
    anyhow::Result<(usize, usize)>,
    anyhow::Result<(usize, usize)>,
) {
    let supervisors = recycle_supervisors();
    let controllers = recycle_controllers();
    (supervisors, controllers)
}

#[cfg(test)]
mod tests {
    use super::{recycle_fleet_with, recycle_on_update_enabled};
    use std::cell::RefCell;
    use std::rc::Rc;

    #[test]
    fn update_recycles_supervisors_then_all_controllers_even_if_supervisor_fanout_fails() {
        let calls = Rc::new(RefCell::new(Vec::new()));
        let supervisor_calls = Rc::clone(&calls);
        let controller_calls = Rc::clone(&calls);
        let (supervisors, controllers) = recycle_fleet_with(
            move || {
                supervisor_calls.borrow_mut().push("supervisors");
                anyhow::bail!("supervisor fanout failed")
            },
            move || {
                controller_calls.borrow_mut().push("controllers");
                Ok((3, 0))
            },
        );

        assert!(supervisors.is_err());
        assert_eq!(controllers.unwrap(), (3, 0));
        assert_eq!(&*calls.borrow(), &["supervisors", "controllers"]);
    }

    #[test]
    fn recycle_after_update_is_default_on_and_falsey_opt_out() {
        let prior = std::env::var("AGENT_DOC_RECYCLE_ON_INSTALL").ok();
        unsafe { std::env::remove_var("AGENT_DOC_RECYCLE_ON_INSTALL") };
        assert!(recycle_on_update_enabled(), "default must be ON");
        for falsey in ["0", "false", "no", "off", "OFF", " False "] {
            unsafe { std::env::set_var("AGENT_DOC_RECYCLE_ON_INSTALL", falsey) };
            assert!(
                !recycle_on_update_enabled(),
                "falsey value {falsey:?} must opt out"
            );
        }
        for truthy in ["1", "true", "yes", "on", "anything"] {
            unsafe { std::env::set_var("AGENT_DOC_RECYCLE_ON_INSTALL", truthy) };
            assert!(
                recycle_on_update_enabled(),
                "truthy/other value {truthy:?} stays ON"
            );
        }
        unsafe {
            match prior {
                Some(value) => std::env::set_var("AGENT_DOC_RECYCLE_ON_INSTALL", value),
                None => std::env::remove_var("AGENT_DOC_RECYCLE_ON_INSTALL"),
            }
        }
    }
}
