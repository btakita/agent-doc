//! Exhaustive no-guess contract for legacy Remote Dev evidence (GH #343).

use agent_doc_editor_surface::SurfaceColumn;
use agent_doc_editor_surface::remote_layout::{
    RemoteClientEditors, RemoteLayoutEvidence, RemoteLayoutMemory, RemoteLayoutSource,
};

const DOCS: [&str; 4] = ["a.md", "b.md", "c.md", "d.md"];

fn evidence(visible: &[&str], selected: &[&str], open: &[&str]) -> RemoteLayoutEvidence {
    let owned = |files: &[&str]| files.iter().map(|file| (*file).to_string()).collect();
    RemoteLayoutEvidence {
        clients: vec![RemoteClientEditors {
            visible: owned(visible),
            selected: owned(selected),
            open: owned(open),
        }],
        focused: selected
            .first()
            .map(|file| (*file).to_string())
            .into_iter()
            .collect(),
    }
}

#[test]
fn every_flat_observation_preserves_or_defers_structure() {
    let snapshots = [
        Vec::new(),
        vec![SurfaceColumn::new(["a.md"])],
        vec![SurfaceColumn::new(["b.md"]), SurfaceColumn::new(["a.md"])],
        vec![
            SurfaceColumn::new(["a.md", "c.md"]),
            SurfaceColumn::new(["d.md"]),
        ],
    ];
    let mut checked = 0usize;
    for columns in snapshots {
        for visible_mask in 0u8..16 {
            let visible: Vec<&str> = DOCS
                .iter()
                .enumerate()
                .filter_map(|(index, file)| (visible_mask & (1 << index) != 0).then_some(*file))
                .collect();
            for selected_mask in 0u8..16 {
                let selected: Vec<&str> = DOCS
                    .iter()
                    .enumerate()
                    .filter_map(|(index, file)| {
                        (selected_mask & (1 << index) != 0).then_some(*file)
                    })
                    .collect();
                let input = evidence(&visible, &selected, &DOCS);
                let memory = RemoteLayoutMemory {
                    columns: columns.clone(),
                    focused: None,
                };
                let (next, resolution) = memory.advance(&input);
                assert_eq!(next.columns, columns);
                assert_eq!(resolution.columns, columns);
                assert_eq!(
                    resolution.source,
                    if columns.is_empty() {
                        RemoteLayoutSource::Unknown
                    } else {
                        RemoteLayoutSource::RetainedRemoteColumns
                    }
                );
                checked += 1;
            }
        }
    }
    assert_eq!(checked, 1_024);
}

#[test]
fn timing_and_repetition_never_upgrade_flat_evidence() {
    let mut memory = RemoteLayoutMemory::default();
    let sequence = [
        evidence(&["a.md", "b.md"], &["a.md"], &DOCS),
        evidence(&["b.md", "a.md"], &["b.md"], &DOCS),
        evidence(&["a.md", "c.md"], &["c.md"], &DOCS),
        evidence(&["a.md", "b.md", "c.md"], &["c.md"], &DOCS),
    ];
    for input in sequence.iter().cycle().take(100_000) {
        let (next, resolution) = memory.advance(input);
        assert!(next.columns.is_empty());
        assert!(resolution.columns.is_empty());
        memory = next;
    }
}
