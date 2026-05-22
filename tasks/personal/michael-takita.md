---
agent_doc_session: 361544a1-7f33-41bb-ba87-a8cacc3944b4
agent_doc_format: template
agent_doc_write: crdt
agent: codex
claude_model: opus
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of agent-loop/tasks/agent-doc/agent-doc-bugs2.md
  '#code-review': Please review tsift codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix. '#follow-up-backlog'
  '#next-steps': Any follow-up items to place in the backlog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
❯ Tell me more about JAK26167F positive myeloproliferative neoplasm.
### Re: JAK2 V617F-positive myeloproliferative neoplasm — gpt-5

I’m assuming “JAK26167F” means **JAK2 V617F**, the common acquired mutation seen in Philadelphia-negative myeloproliferative neoplasms (MPNs). This is general medical information, not a diagnosis or a substitute for a hematologist’s advice.

A JAK2 V617F-positive MPN means a clone of blood-forming bone marrow cells has an activating mutation in the JAK2 signaling pathway. That can push the marrow to overproduce one or more blood cell lines. The “positive” result supports a clonal blood disorder, but it does not by itself identify which MPN it is. The main possibilities are:

- **Polycythemia vera (PV):** too many red blood cells, often with high hematocrit, sometimes with high white cells or platelets. Major concerns include blood clots, bleeding, spleen enlargement, and less commonly progression to myelofibrosis or acute leukemia.
- **Essential thrombocythemia (ET):** too many platelets. It may be asymptomatic or cause headaches, burning/tingling/redness in the hands or feet, and vision or hearing changes. The major concern is clotting, and sometimes bleeding.
- **Primary myelofibrosis (PMF), including prefibrotic or overt forms:** abnormal blood formation and marrow scarring. Symptoms can include anemia, fatigue, enlarged spleen or fullness under the left ribs, night sweats, weight loss, bone pain, shortness of breath, bruising, or bleeding.

The usual workup combines CBC trends, peripheral smear, iron studies/ferritin, erythropoietin level when PV is possible, mutation testing such as JAK2/CALR/MPL and often a broader myeloid NGS panel, bone marrow biopsy findings, spleen exam or imaging, and BCR-ABL1 testing to exclude chronic myeloid leukemia. JAK2 allele burden can add context, but it is not the whole diagnosis or treatment plan.

Treatment depends more on the exact subtype and risk category than on JAK2 positivity alone.

- **PV** treatment often aims to keep hematocrit controlled and reduce clot risk: phlebotomy, low-dose aspirin when appropriate, and cytoreductive therapy such as hydroxyurea or interferon/ropeginterferon in higher-risk or selected patients. Ruxolitinib can be used in some hydroxyurea-resistant or intolerant cases.
- **ET** may be monitored if low risk and asymptomatic. Aspirin is common when clot-prevention or vasomotor-symptom benefit outweighs bleeding risk. Higher-risk patients may need platelet-lowering treatment such as hydroxyurea, interferon, or anagrelide.
- **Myelofibrosis** ranges from observation to symptom-directed anemia care, JAK inhibitors, clinical trials, and, in selected fit higher-risk patients, allogeneic stem cell transplant. In the U.S., four JAK inhibitors are approved for myelofibrosis: ruxolitinib, fedratinib, pacritinib, and momelotinib.

Useful questions for the hematologist:

1. Which exact MPN subtype is diagnosed: PV, ET, prefibrotic MF, overt MF, or MPN-unclassifiable?
2. What are the current CBC values and trends, and is hematocrit, platelet count, or white blood cell count the main issue?
3. Was BCR-ABL1 negative? Were CALR/MPL and a broader myeloid mutation panel checked?
4. Is a bone marrow biopsy needed, or has it already established the subtype?
5. What is the clotting and bleeding risk category?
6. Should low-dose aspirin be used, or avoided because of bleeding risk or acquired von Willebrand disease risk?
7. Is watchful waiting reasonable, or is cytoreduction/JAK inhibitor treatment indicated?
8. What symptoms should trigger urgent care?

Urgent symptoms such as sudden weakness or numbness, facial droop, chest pain, severe shortness of breath, new one-sided leg swelling or pain, coughing blood, severe headache with vision change, or uncontrolled bleeding should be treated as urgent/emergency symptoms because MPNs can increase both clotting and bleeding risk.

Sources I used: [NCI PDQ patient overview and treatment](https://www.cancer.gov/types/myeloproliferative/patient/chronic-treatment-pdq), [Blood Cancer United/LLS MPN overview](https://bloodcancerunited.org/blood-cancer/myeloproliferative-neoplasms-mpns), [NCCN MPN guideline abstract via PubMed](https://pubmed.ncbi.nlm.nih.gov/36075392/), [Merck Manual ET risk/treatment summary](https://www.merckmanuals.com/professional/hematology-and-oncology/myeloproliferative-disorders/essential-thrombocythemia), and [MD Anderson MPN treatment research page](https://www.mdanderson.org/research/departments-labs-institutes/programs-centers/clinical-research-center-for-myeloproliferative-neoplasms/current-mpn-research.html).
<!-- agent:boundary:608fbc3e -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
<!-- /agent:backlog -->

## Icebox

<!-- agent:icebox -->
<!-- /agent:icebox -->
