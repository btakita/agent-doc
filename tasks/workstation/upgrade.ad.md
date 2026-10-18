---
agent_doc_session: 010b6450-eeac-40f6-9ae9-b5e2adbddc36
agent_doc_format: template
agent_doc_write: crdt
agent: codex
resume:
  claude: 80050dae-ffc1-48f6-bb8c-17c794ecb6ac
  codex: 01a14d5c-352b-7781-b889-76815eb85c88
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->

### Re: Codex session selection — gpt-5 · 2026-10-18T00:54-04:00

The document is now configured to use Codex (`agent: codex`). Its queue and backlog are empty, so there is no workstation-upgrade action to execute yet.

Session-accretion is advisory-only: two recent no-op closeouts were detected. I left the exchange history intact because compaction requires your explicit approval.
<!-- agent:boundary:2bf644f7 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue priority subagents -->
<!-- /agent:queue -->

## Todo

<!-- agent:todo priority -->
<!-- /agent:todo -->

## Backlog

<!-- agent:backlog priority queue -->
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
<!-- /agent:review -->

## Icebox

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Done

<!-- agent:done -->
<!-- /agent:done -->

## Notes

<!-- agent:notes -->
# Workstation upgrade context from chat

## User intent

The user plans to add an NVIDIA RTX PRO 6000 Blackwell Workstation Edition to the existing RTX 5090 workstation. Populate `tasks/workstation/upgrade.ad.md` with these verified facts, conclusions, official links, and the remaining decisions so the user can continue the discussion there.

## Verified current system

- CPU: AMD Ryzen 9 9950X3D, 16 cores / 32 threads, 170 W rated TDP.
- Memory: 186 GiB usable system RAM (nominally 192 GB installed).
- GPU: NVIDIA GeForce RTX 5090 with 32,607 MiB reported VRAM.
  - Current/default power limit: 575 W.
  - Hardware maximum power limit: 600 W.
  - PCI address: `01:00.0`.
  - MSI subsystem vendor (`1462`). Exact cooler/card model remains unknown.
- Motherboard: MSI PRO X870E-P WIFI (`MS-7E70`, product version 2.0).
- PSU: ASUS ROG Strix 1200W Platinum, fully modular, ATX 3.1.
  - Rated output: 1,200 W on the 12 V rail.
  - Cabling: one native PCIe 16-pin cable plus four PCIe 6+2-pin cables.
  - Official specification: https://rog.asus.com/us/power-supply-units/rog-strix/rog-strix-1200p-gaming/spec/

## Proposed second GPU

The intended card is the 96 GB RTX PRO 6000 Blackwell Workstation Edition, not the older 48 GB RTX 6000 Ada.

- 96 GB GDDR7 ECC.
- 600 W total board power.
- PCIe 5.0 x16 interface.
- One PCIe CEM5 16-pin power connector.
- 12.0 inches long, 5.4 inches high, dual-slot, extended-height card.
- Double-flow-through cooling.
- Official specification: https://www.nvidia.com/content/dam/en-zz/Solutions/data-center/rtx-pro-6000-blackwell-workstation-edition/workstation-blackwell-rtx-pro-6000-workstation-edition-nvidia-us-3519208-web.pdf

## Power conclusion

Approximate component budget:

- RTX 5090: 575 W.
- RTX PRO 6000 Blackwell: 600 W.
- Ryzen 9 9950X3D: 170 W rated TDP.
- Board, RAM, storage, cooling, and USB: approximately 75–150 W.
- Estimated system total: approximately 1,420–1,500 W.

The existing 1,200 W PSU is not adequate. The GPUs alone can request 1,175 W, leaving only 25 W for everything else, and the PSU has only one native 16-pin output.

A 1,600 W ATX 3.1 PSU with two native 12V-2x6 outputs is the practical minimum. It is suitable when simultaneous workloads are power-limited, especially if the 5090 is reduced from 575 W toward its locally reported 400 W minimum. With both GPUs unrestricted and CPU/platform heavily loaded, it has only about 100–180 W estimated headroom, too little for the preferred sustained-compute configuration.

Current 1,600 W candidates with two native 12V-2x6 outputs:

- MSI MEG Ai1600T PCIE5: https://www.msi.com/Power-Supply/MEG-Ai1600T-PCIE5/
- ASUS ROG Thor 1600W Titanium III: https://rog.asus.com/power-supply-units/rog-thor/rog-thor-1600t3-gaming-evo/spec/
- Seasonic PRIME TX-1600 Noctua Edition: https://seasonic.com/product/prime-tx-1600-noctua-edition/

For unrestricted simultaneous GPU and CPU workloads, prefer 1,800–2,000 W with two native 12V-2x6 cables. A 2,200 W unit such as Seasonic PRIME PX-2200 provides margin but requires 200–240 V input: https://seasonic.com/wp-content/uploads/2024/06/ATX-3.1-PRIME-PX-2200.pdf

At roughly 1.4–1.5 kW DC output, wall draw is higher after conversion losses. Plan on a dedicated 20 A / 120 V circuit or preferably 240 V after verifying the chosen PSU input specification. A shared 15 A / 120 V circuit is not a comfortable sustained-compute configuration.

## Motherboard conclusion

The current board can enumerate two GPUs but is not a good full-performance combined-GPU platform:

- `PCI_E1`: PCIe 5.0 x16 directly from the CPU.
- `PCI_E3`: full-length connector operating only at PCIe 4.0 x4 through the X870E chipset.
- Official specification: https://www.msi.com/Motherboard/PRO-X870E-P-WIFI/Specification

Consequences:

- Independent jobs, one per GPU: potentially workable. The x4 link mainly affects loading and host transfers once each workload fits within its GPU.
- One model divided across both GPUs: not recommended. PCIe 4.0 x4 is about 8 GB/s theoretical one-way and traverses the chipset, making synchronization and tensor/model-parallel transfers a likely bottleneck.
- The 32 GB and 96 GB memories remain separate; software must explicitly partition/offload. They do not automatically form a unified 128 GB pool.
- For a combined workload, the RTX PRO 6000 likely belongs in the CPU-connected top slot and the 5090 in the constrained x4 slot.
- Populate the board's supplemental `PCIE_PWR` 8-pin input, but it does not replace either GPU's dedicated 16-pin cable.

For serious model parallelism, replace the motherboard with two CPU-connected slots at PCIe 5.0 x8/x8:

- ASUS ProArt X870E-Creator WIFI: https://dlcdnets.asus.com/pub/ASUS/mb/SocketAM5/ProArt_X870E-CREATOR_WIFI/E23930_ProArt_X870E-CREATOR_WIFI_EM_WEB.pdf
  - Two CPU-connected PCIe 5.0 slots at x8/x8.
  - Respect its `M.2_2` sharing rules to retain x8/x8.
- Gigabyte X870E AORUS XTREME AI TOP: https://www.gigabyte.com/za/Motherboard/X870E-AORUS-XTREME-AI-TOP-rev-1x/sp
  - Two CPU-connected slots operating at x8/x8 when both are populated.

## Mechanical and thermal checks still required

The exact case and exact MSI RTX 5090 model remain unknown.

1. Confirm the case supports the RTX PRO card's 12-inch length and 5.4-inch extended height, including safe 12V-2x6 cable clearance.
2. Confirm the 5090's slot thickness does not cover the lower full-length slot.
3. Confirm intake space remains around both cards; together they can dissipate about 1,175 W as GPU heat.
4. Use appropriate support brackets/chassis retention.
5. Never reuse modular cables from the ASUS PSU with a replacement PSU unless explicitly listed as compatible by the new PSU manufacturer.
6. Fully seat both native 12V-2x6 connectors and avoid tight bends immediately at the plugs.

## Recommended paths

- Independent workloads plus power limiting: a high-quality 1,600 W ATX 3.1 PSU with two native 12V-2x6 outputs may suffice; test the existing motherboard with the second GPU at PCIe 4.0 x4.
- One workload spanning both GPUs: upgrade to a true CPU-connected x8/x8 motherboard and use an 1,800–2,000 W PSU with suitable mains power.
- No power limits, long sustained compute, maximum margin: use the higher-wattage path and 240 V where required or beneficial.

## Questions for the next turn

1. Will the cards run independent jobs, inference with layer offload, or tensor/model-parallel training/inference?
2. What is the case make/model and exact MSI RTX 5090 model?
3. Is the outlet on a dedicated 15 A, dedicated 20 A, or 240 V circuit?
4. Is replacing the motherboard acceptable?
5. Is power-limiting the RTX 5090 while both cards run acceptable?
<!-- /agent:notes -->
