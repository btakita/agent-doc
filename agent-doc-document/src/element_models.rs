//! Composition of per-element realtime models into the document model.

use agent_doc_element::{
    ElementCompositionRole, ElementDescriptor, ElementRealtimeModel, ElementSchedulingRole,
};

/// Pure document-level view of available element models.
///
/// This type intentionally contains no IO and no plugin loader. A later runtime
/// registry can layer plugin-provided descriptors on top of the built-ins before
/// constructing this composed model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentElementModels {
    descriptors: Vec<ElementDescriptor>,
}

impl Default for DocumentElementModels {
    fn default() -> Self {
        Self::built_in()
    }
}

impl DocumentElementModels {
    pub fn built_in() -> Self {
        Self {
            descriptors: agent_doc_element_registry::built_in_elements().to_vec(),
        }
    }

    pub fn with_descriptors(descriptors: Vec<ElementDescriptor>) -> Self {
        Self { descriptors }
    }

    pub fn descriptors(&self) -> &[ElementDescriptor] {
        &self.descriptors
    }

    pub fn find_known(&self, name: &str) -> Option<ElementDescriptor> {
        self.descriptors
            .iter()
            .copied()
            .find(|descriptor| descriptor.matches_name(name))
    }

    pub fn descriptor_for(&self, name: &str) -> ElementDescriptor {
        self.find_known(name)
            .unwrap_or_else(|| agent_doc_element_registry::descriptor_for(name))
    }

    pub fn runnable_work_sources(&self) -> Vec<ElementDescriptor> {
        self.descriptors
            .iter()
            .copied()
            .filter(|descriptor| {
                descriptor.scheduling_role == ElementSchedulingRole::RunnableWorkSource
            })
            .collect()
    }

    pub fn queue_consumers(&self) -> Vec<ElementDescriptor> {
        self.descriptors
            .iter()
            .copied()
            .filter(|descriptor| {
                descriptor.realtime_model == ElementRealtimeModel::Queue
                    || descriptor.composition_role == ElementCompositionRole::Consumer
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_doc_element::{ElementAuthority, ElementWritePolicy};

    #[test]
    fn document_model_composes_built_in_element_models() {
        let model = DocumentElementModels::built_in();
        assert!(model.find_known("exchange").is_some());
        assert!(model.find_known("queue").is_some());
        assert!(model.find_known("backlog").is_some());
        assert!(model.find_known("pending").is_some());
        assert!(model.find_known("icebox").is_some());
        assert!(model.find_known("signals").is_some());
        assert!(model.find_known("boundary").is_some());
    }

    #[test]
    fn unknown_component_uses_operator_authoritative_fallback() {
        let model = DocumentElementModels::built_in();
        let descriptor = model.descriptor_for("operator-notes");
        assert_eq!(descriptor.name, "__unknown__");
        assert_eq!(
            descriptor.authority,
            ElementAuthority::SharedOperatorAuthoritative
        );
        assert_eq!(descriptor.write_policy, ElementWritePolicy::MergeOnly);
        assert_eq!(descriptor.realtime_model, ElementRealtimeModel::Unknown);
    }

    #[test]
    fn backlog_is_runnable_source_but_icebox_is_parked() {
        let model = DocumentElementModels::built_in();
        let runnable: Vec<&str> = model
            .runnable_work_sources()
            .iter()
            .map(|descriptor| descriptor.name)
            .collect();
        assert_eq!(runnable, vec!["backlog"]);

        let icebox = model.find_known("icebox").unwrap();
        assert_eq!(
            icebox.scheduling_role,
            ElementSchedulingRole::ParkedWorkSource
        );
        assert_ne!(
            icebox.scheduling_role,
            ElementSchedulingRole::RunnableWorkSource
        );
    }
}
